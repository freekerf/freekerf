//! Laying a [`TextSpec`] out: the lines shaped in their font (characters a
//! single-line font lacks come from [`DEFAULT_FONT`]), aligned, spaced and styled;
//! straight from the origin, bent on a circle or along a path; then the effects of
//! outline fonts (weld, stencil, outline). The result is in mm, Y up: at the
//! origin (the first baseline starts, is centred or ends there) for a straight or
//! bent text, in document coordinates for a text on a path.

use freekerf_document::{TextAlign, TextSpec};
use freekerf_geometry::boolean::{self, Join, Op};
use freekerf_geometry::{Matrix, Path, Point, Segment};

use crate::TextError;
use crate::along::Along;
use crate::face::Metrics;
use crate::fonts::{DEFAULT_FONT, Fonts, StyledFace};

/// Tolerance of the polygons the effects work on, mm.
const POLYGON_TOLERANCE_MM: f64 = 0.01;
/// Slant of a drawn italic: tan 12°.
const ITALIC_SLANT: f64 = 0.212_556_56;
/// A drawn bold outline grows by this, times the height.
const BOLD_OUTLINE: f64 = 0.03;
/// A drawn bold single-line letter is drawn again this far to the right, times
/// the height.
const BOLD_STROKE: f64 = 0.05;
/// Step of the decorations that follow a path, mm.
const DECORATION_STEP_MM: f64 = 0.5;
/// Largest number of characters of a text.
pub const MAX_CHARS: usize = 10_000;

/// A glyph placed on its line (line coordinates: x from the line's start, y from
/// its baseline), mm.
struct Placed {
    paths: Vec<Path>,
    /// Pen position.
    pen: f64,
    advance: f64,
    x_offset: f64,
    y_offset: f64,
    space: bool,
}

struct Line {
    glyphs: Vec<Placed>,
    width: f64,
}

/// Checks the numbers of a text.
///
/// # Errors
/// The first value out of range.
pub fn check(spec: &TextSpec) -> Result<(), TextError> {
    let bad = |what: &str| Err(TextError::InvalidSpec(what.into()));
    if !(spec.height_mm.is_finite() && spec.height_mm > 0.0) {
        return bad("height_mm must be positive");
    }
    if !(spec.line_spacing.is_finite() && spec.line_spacing > 0.0) {
        return bad("line_spacing must be positive");
    }
    if !(spec.width_scale.is_finite() && spec.width_scale > 0.0) {
        return bad("width_scale must be positive");
    }
    if !spec.letter_spacing_mm.is_finite() {
        return bad("letter_spacing_mm must be finite");
    }
    if spec.outline_mm.is_some_and(|d| !d.is_finite() || d == 0.0) {
        return bad("outline_mm must be finite and not 0");
    }
    if spec.stencil_mm.is_some_and(|w| !(w.is_finite() && w > 0.0)) {
        return bad("stencil_mm must be positive");
    }
    if spec.arc_radius_mm.is_some_and(|r| !r.is_finite() || r == 0.0) {
        return bad("arc_radius_mm must be finite and not 0");
    }
    if let Some(p) = &spec.path
        && !(p.offset_mm.is_finite() && p.distance_mm.is_finite())
    {
        return bad("the path's offset and distance must be finite");
    }
    if spec.text.chars().count() > MAX_CHARS {
        return Err(TextError::InvalidSpec(format!(
            "a text has at most {MAX_CHARS} characters"
        )));
    }
    Ok(())
}

/// The glyph matrix of a face: font units to mm, the width scale and a drawn slant.
fn glyph_matrix(k: f64, width_scale: f64, slant: bool) -> Matrix {
    Matrix::new(
        k * width_scale,
        0.0,
        if slant { ITALIC_SLANT * k } else { 0.0 },
        k,
        0.0,
        0.0,
    )
}

/// One line shaped: runs of the main face, and of the fallback for characters the
/// main face lacks.
fn shape_line(text: &str, main: &StyledFace, fallback: Option<&StyledFace>, spec: &TextSpec) -> Vec<Placed> {
    let h = spec.height_mm;
    let face_of = |c: char| match fallback {
        Some(f) if !c.is_whitespace() && !main.face.covers(c) && f.face.covers(c) => f,
        _ => main,
    };
    let mut runs: Vec<(&StyledFace, String)> = Vec::new();
    for c in text.chars() {
        let f = face_of(c);
        match runs.last_mut() {
            Some((last, s)) if std::ptr::eq(*last, f) => s.push(c),
            _ => runs.push((f, c.to_string())),
        }
    }
    let main_metrics = main.face.metrics();
    // capitals of the fallback as high as the main face draws its own
    let drawn = h * main_metrics.drawn_cap_height / main_metrics.cap_height;
    let mut out = Vec::new();
    for (f, run) in runs {
        let m = f.face.metrics();
        let k = if std::ptr::eq(f, main) {
            h / m.cap_height
        } else {
            drawn / m.cap_height
        };
        let matrix = glyph_matrix(k, spec.width_scale, f.synthetic_italic);
        let stroke_bold = f.synthetic_bold && f.face.single_line();
        let outline_bold = f.synthetic_bold && !f.face.single_line();
        for g in f.face.shape(&run) {
            let mut paths: Vec<Path> = g
                .paths
                .iter()
                .map(|p| p.transformed(&matrix, POLYGON_TOLERANCE_MM))
                .collect();
            let mut advance = g.x_advance * k * spec.width_scale;
            if stroke_bold && !paths.is_empty() {
                let shift = BOLD_STROKE * h;
                let copies: Vec<Path> = paths
                    .iter()
                    .map(|p| {
                        let mut q = p.clone();
                        q.translate(shift, 0.0);
                        q
                    })
                    .collect();
                paths.extend(copies);
                advance += shift;
            }
            if outline_bold {
                advance += 2.0 * BOLD_OUTLINE * h;
            }
            out.push(Placed {
                paths,
                pen: 0.0,
                advance,
                x_offset: g.x_offset * k * spec.width_scale,
                y_offset: g.y_offset * k,
                space: g.space,
            });
        }
    }
    out
}

/// Pen positions with `extra_space` added after each space and `extra_gap` after
/// every glyph; returns the width (without the spacing after the last glyph).
fn set_pens(glyphs: &mut [Placed], letter_spacing: f64, extra_space: f64, extra_gap: f64) -> f64 {
    let mut pen = 0.0;
    let n = glyphs.len();
    let mut width = 0.0;
    for (i, g) in glyphs.iter_mut().enumerate() {
        g.pen = pen;
        width = pen + g.advance;
        if i + 1 < n {
            pen = width + letter_spacing + extra_gap + if g.space { extra_space } else { 0.0 };
        }
    }
    width
}

/// Where the baseline goes.
enum Baseline {
    Straight,
    Along {
        along: Along,
        /// Bent on a circle (always centred on its top or bottom).
        arc: bool,
        offset: f64,
        distance: f64,
    },
}

/// A circle through the origin whose top (radius > 0) or bottom (< 0) is at the
/// origin, drawn so that the text reads left to right there.
fn arc_path(radius: f64) -> Path {
    let r = radius.abs();
    if radius > 0.0 {
        let c = Point::new(0.0, -r);
        let mut p = Path::new(Point::new(0.0, -2.0 * r));
        p.segments.push(Segment::arc(Point::ZERO, c, false));
        p.segments.push(Segment::arc(Point::new(0.0, -2.0 * r), c, false));
        p.closed = true;
        p
    } else {
        let c = Point::new(0.0, r);
        let mut p = Path::new(Point::new(0.0, 2.0 * r));
        p.segments.push(Segment::arc(Point::ZERO, c, true));
        p.segments.push(Segment::arc(Point::new(0.0, 2.0 * r), c, true));
        p.closed = true;
        p
    }
}

/// The matrix that puts a point `(x, y)` of line coordinates, at distance `xc`
/// along the line, on the path: `y + dn` from it, turned with it.
fn on_path(along: &Along, s: f64, half: f64, xc: f64, dn: f64) -> Matrix {
    let p = along.point(s);
    let (sin, cos) = along.angle(s, half).sin_cos();
    Matrix::new(
        cos,
        sin,
        -sin,
        cos,
        p.x - cos * xc - sin * dn,
        p.y - sin * xc + cos * dn,
    )
}

/// A decoration (underline, strike) of a line: `y` from the baseline, `t` thick.
/// Single-line: a stroke; outline: a closed band.
fn decoration(baseline: &Baseline, x0: f64, width: f64, y: f64, t: f64, dn: f64, single: bool) -> Path {
    match baseline {
        Baseline::Straight => {
            if single {
                Path::polyline(&[Point::new(x0, y), Point::new(x0 + width, y)], false)
            } else {
                freekerf_document::rect_path(x0, y - t / 2.0, width, t)
            }
        }
        Baseline::Along { along, .. } => {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss
            )]
            let steps = ((width / DECORATION_STEP_MM).ceil() as usize).max(1);
            let side = |off: f64| -> Vec<Point> {
                (0..=steps)
                    .map(|i| {
                        #[allow(clippy::cast_precision_loss)]
                        let s = x0 + width * (i as f64) / (steps as f64);
                        let p = along.point(s);
                        let (sin, cos) = along.angle(s, DECORATION_STEP_MM / 2.0).sin_cos();
                        Point::new(p.x - sin * (dn + off), p.y + cos * (dn + off))
                    })
                    .collect()
            };
            if single {
                Path::polyline(&side(y), false)
            } else {
                let mut pts = side(y + t / 2.0);
                pts.extend(side(y - t / 2.0).into_iter().rev());
                Path::polyline(&pts, true)
            }
        }
    }
}

fn polygons(paths: &[Path]) -> Vec<Vec<Point>> {
    paths
        .iter()
        .filter(|p| p.closed)
        .map(|p| {
            let mut pts = p.to_polyline(POLYGON_TOLERANCE_MM);
            if pts.len() > 1 && pts.first() == pts.last() {
                pts.pop();
            }
            pts
        })
        .filter(|p| p.len() > 2)
        .collect()
}

fn closed(polys: Vec<Vec<Point>>) -> Vec<Path> {
    polys.into_iter().map(|p| Path::polyline(&p, true)).collect()
}

fn geometry(e: impl std::fmt::Display) -> TextError {
    TextError::Geometry(e.to_string())
}

/// `pt` inside `poly` (even-odd ray cast).
fn inside(poly: &[Point], pt: Point) -> bool {
    let mut c = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + n - 1) % n]);
        if (a.y > pt.y) != (b.y > pt.y) && pt.x < (b.x - a.x) * (pt.y - a.y) / (b.y - a.y) + a.x {
            c = !c;
        }
    }
    c
}

fn bounds(poly: &[Point]) -> freekerf_geometry::Rect {
    let mut r = freekerf_geometry::Rect::EMPTY;
    for p in poly {
        r.add(*p);
    }
    r
}

/// Stencil bridges: a vertical bar `width` wide through the middle of every hole,
/// across the shape around it, is cut away (so the hole's island stays held).
/// `polys` are a union's result: outer contours counterclockwise, holes clockwise.
fn stencil(polys: Vec<Vec<Point>>, width: f64) -> Result<Vec<Vec<Point>>, TextError> {
    let mut bars = Vec::new();
    for hole in polys.iter().filter(|p| boolean::signed_area(p) < 0.0) {
        let hb = bounds(hole);
        let Some(outer) = polys
            .iter()
            .filter(|p| boolean::signed_area(p) > 0.0 && inside(p, hole[0]))
            .min_by(|a, b| boolean::signed_area(a).total_cmp(&boolean::signed_area(b)))
        else {
            continue;
        };
        let ob = bounds(outer);
        let xc = (hb.min_x + hb.max_x) / 2.0;
        let (x0, x1) = (xc - width / 2.0, xc + width / 2.0);
        let (y0, y1) = (ob.min_y - width, ob.max_y + width);
        bars.push(vec![
            Point::new(x0, y0),
            Point::new(x1, y0),
            Point::new(x1, y1),
            Point::new(x0, y1),
        ]);
    }
    if bars.is_empty() {
        return Ok(polys);
    }
    boolean::boolean(&polys, &bars, Op::Difference, boolean::DEFAULT_SCALE).map_err(geometry)
}

/// The geometry of `spec`.
///
/// # Errors
/// Values out of range ([`check`]), an unavailable font, or a geometry failure.
#[allow(clippy::too_many_lines)]
pub fn layout(spec: &TextSpec, fonts: &Fonts) -> Result<Vec<Path>, TextError> {
    check(spec)?;
    let h = spec.height_mm;
    let main = fonts.face(&spec.font, spec.bold, spec.italic)?;
    let fallback = (main.face.single_line() && !spec.font.eq_ignore_ascii_case(DEFAULT_FONT))
        .then(|| fonts.face(DEFAULT_FONT, spec.bold, spec.italic).ok())
        .flatten();
    let metrics: Metrics = *main.face.metrics();
    let k = h / metrics.cap_height;
    let single = main.face.single_line();
    let pitch = spec.line_spacing * metrics.line_height() * k;

    let mut lines: Vec<Line> = spec
        .text
        .split('\n')
        .map(|l| {
            let mut glyphs = shape_line(l.trim_end_matches('\r'), &main, fallback.as_ref(), spec);
            let width = set_pens(&mut glyphs, spec.letter_spacing_mm, 0.0, 0.0);
            Line { glyphs, width }
        })
        .collect();

    let baseline = if let Some(tp) = &spec.path {
        let path = if tp.flip {
            tp.path.reversed()
        } else {
            tp.path.clone()
        };
        Along::new(&path).map_or(Baseline::Straight, |along| Baseline::Along {
            along,
            arc: false,
            offset: tp.offset_mm,
            distance: tp.distance_mm,
        })
    } else if let Some(r) = spec.arc_radius_mm {
        Along::new(&arc_path(r)).map_or(Baseline::Straight, |along| Baseline::Along {
            along,
            arc: true,
            offset: 0.0,
            distance: 0.0,
        })
    } else {
        Baseline::Straight
    };

    // justified: straight lines (but the last) as wide as the widest, a text on a
    // path as long as the path
    let widest = lines.iter().map(|l| l.width).fold(0.0, f64::max);
    let count = lines.len();
    for (i, line) in lines.iter_mut().enumerate() {
        if spec.align != TextAlign::Justify || line.glyphs.is_empty() {
            continue;
        }
        match &baseline {
            Baseline::Straight => {
                let spaces = line.glyphs[..line.glyphs.len() - 1]
                    .iter()
                    .filter(|g| g.space)
                    .count();
                if i + 1 < count && spaces > 0 && widest > line.width {
                    #[allow(clippy::cast_precision_loss)]
                    let extra = (widest - line.width) / spaces as f64;
                    line.width = set_pens(&mut line.glyphs, spec.letter_spacing_mm, extra, 0.0);
                }
            }
            Baseline::Along {
                along,
                arc: false,
                offset,
                ..
            } if line.glyphs.len() > 1 => {
                #[allow(clippy::cast_precision_loss)]
                let extra = (along.length() - offset - line.width) / (line.glyphs.len() - 1) as f64;
                line.width = set_pens(&mut line.glyphs, spec.letter_spacing_mm, 0.0, extra);
            }
            Baseline::Along { .. } => {}
        }
    }

    let mut out: Vec<Path> = Vec::new();
    let mut decorations: Vec<Path> = Vec::new();
    let decorate = |y_mid: f64, thick: f64| (y_mid * k, thick * k);
    let mut marks: Vec<(f64, f64)> = Vec::new();
    if spec.underline {
        marks.push(decorate(metrics.underline_position, metrics.underline_thickness));
    }
    if spec.strikethrough {
        marks.push(decorate(metrics.strikeout_position, metrics.strikeout_thickness));
    }
    for (i, line) in lines.iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let down = i as f64 * pitch;
        match &baseline {
            Baseline::Straight => {
                let x0 = match spec.align {
                    TextAlign::Left | TextAlign::Justify => 0.0,
                    TextAlign::Center => -line.width / 2.0,
                    TextAlign::Right => -line.width,
                };
                for g in &line.glyphs {
                    for p in &g.paths {
                        let mut q = p.clone();
                        q.translate(x0 + g.pen + g.x_offset, g.y_offset - down);
                        out.push(q);
                    }
                }
                if !line.glyphs.is_empty() {
                    for &(y, t) in &marks {
                        decorations.push(decoration(&baseline, x0, line.width, y - down, t, 0.0, single));
                    }
                }
            }
            Baseline::Along {
                along,
                arc,
                offset,
                distance,
            } => {
                let len = along.length();
                let s0 = if *arc {
                    (len - line.width) / 2.0
                } else {
                    offset
                        + match spec.align {
                            TextAlign::Left | TextAlign::Justify => 0.0,
                            TextAlign::Center => (len - line.width) / 2.0,
                            TextAlign::Right => len - line.width,
                        }
                };
                let dn = distance - down;
                for g in &line.glyphs {
                    let xc = g.pen + g.advance / 2.0;
                    let m = on_path(along, s0 + xc, g.advance / 2.0, xc, dn);
                    for p in &g.paths {
                        let mut q = p.clone();
                        q.translate(g.pen + g.x_offset, g.y_offset);
                        out.push(q.transformed(&m, POLYGON_TOLERANCE_MM));
                    }
                }
                if !line.glyphs.is_empty() {
                    for &(y, t) in &marks {
                        decorations.push(decoration(&baseline, s0, line.width, y, t, dn, single));
                    }
                }
            }
        }
    }

    if single {
        out.extend(decorations);
        if let Some(d) = spec.outline_mm {
            let strokes: Vec<Vec<Point>> = out.iter().map(|p| p.to_polyline(POLYGON_TOLERANCE_MM)).collect();
            let around =
                boolean::offset_open(&strokes, d.abs(), POLYGON_TOLERANCE_MM, boolean::DEFAULT_SCALE)
                    .map_err(geometry)?;
            out.extend(closed(around));
        }
        return Ok(out);
    }

    let reshape = main.synthetic_bold || spec.weld || !decorations.is_empty() || spec.stencil_mm.is_some();
    let mut polys = if reshape || spec.outline_mm.is_some() {
        // the letters' union first: it gives their contours the direction of the
        // decorations (TrueType draws outer contours clockwise, CFF the other way)
        let union = |polys: &[Vec<Point>]| {
            boolean::boolean(polys, &[], Op::Union, boolean::DEFAULT_SCALE).map_err(geometry)
        };
        let mut letters = union(&polygons(&out))?;
        if !decorations.is_empty() {
            letters.extend(polygons(&decorations));
            letters = union(&letters)?;
        }
        letters
    } else {
        Vec::new()
    };
    let round = Join::Round {
        arc_tolerance: POLYGON_TOLERANCE_MM,
    };
    if reshape {
        if main.synthetic_bold {
            polys =
                boolean::offset(&polys, BOLD_OUTLINE * h, round, boolean::DEFAULT_SCALE).map_err(geometry)?;
        }
        if let Some(w) = spec.stencil_mm {
            polys = stencil(polys, w)?;
        }
        out = closed(polys.clone());
    }
    if let Some(d) = spec.outline_mm {
        let around = boolean::offset(&polys, d, round, boolean::DEFAULT_SCALE).map_err(geometry)?;
        out.extend(closed(around));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use freekerf_document::{LEGACY_FONT, TextPath};
    use freekerf_geometry::Rect;

    use super::*;
    use crate::testfont;

    fn bounds_of(paths: &[Path]) -> Rect {
        let mut r = Rect::EMPTY;
        for p in paths {
            r.union(&p.bounds());
        }
        r
    }

    fn spec(text: &str, font: &str) -> TextSpec {
        TextSpec::new(text, 10.0, font)
    }

    #[test]
    fn the_legacy_font_keeps_its_strokes() {
        let fonts = Fonts::builtin();
        let got = layout(&spec("Hi!", LEGACY_FONT), &fonts).unwrap();
        let expected = crate::hershey::text_polylines("Hi!", 0.0, 0.0, 10.0);
        let got: Vec<Vec<(f64, f64)>> = got
            .iter()
            .map(|p| p.to_polyline(0.01).into_iter().map(|q| (q.x, q.y)).collect())
            .collect();
        assert_eq!(got.len(), expected.len());
        for (a, b) in got.iter().flatten().zip(expected.iter().flatten()) {
            assert!((a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9);
        }
        // characters it lacks come from the default font
        assert!(layout(&spec("é", LEGACY_FONT), &fonts).unwrap().len() > 1);
    }

    #[test]
    fn capitals_are_as_high_as_asked_and_lines_align() {
        let fonts = Fonts::builtin();
        let b = bounds_of(&layout(&spec("H", DEFAULT_FONT), &fonts).unwrap());
        assert!((b.max_y - 10.0).abs() < 0.2 && b.min_y.abs() < 0.2, "{b:?}");
        let mut s = spec("HH\nH", DEFAULT_FONT);
        let left = bounds_of(&layout(&s, &fonts).unwrap());
        // the letters' side bearings are about 2 mm
        assert!(left.min_x.abs() < 3.0 && left.min_y < -10.0, "{left:?}");
        s.align = TextAlign::Right;
        let right = bounds_of(&layout(&s, &fonts).unwrap());
        assert!(right.max_x.abs() < 3.0, "{right:?}");
        s.align = TextAlign::Center;
        let center = bounds_of(&layout(&s, &fonts).unwrap());
        assert!((center.min_x + center.max_x).abs() < 1.5, "{center:?}");
        // line spacing and letter spacing
        s.line_spacing = 2.0;
        assert!(bounds_of(&layout(&s, &fonts).unwrap()).min_y < center.min_y - 5.0);
        let mut wide = spec("HH", DEFAULT_FONT);
        let w0 = bounds_of(&layout(&wide, &fonts).unwrap()).width();
        wide.letter_spacing_mm = 3.0;
        assert!((bounds_of(&layout(&wide, &fonts).unwrap()).width() - w0 - 3.0).abs() < 1e-6);
        wide.width_scale = 2.0;
        assert!(bounds_of(&layout(&wide, &fonts).unwrap()).width() > 2.0 * w0);
    }

    #[test]
    fn justified_lines_are_as_wide_as_the_widest() {
        let fonts = Fonts::builtin();
        let mut s = spec("A B C D E\nA B\nA B", DEFAULT_FONT);
        s.align = TextAlign::Justify;
        let paths = layout(&s, &fonts).unwrap();
        let line = |i: usize| {
            let mut one = s.clone();
            one.text = s.text.split('\n').nth(i).unwrap().into();
            bounds_of(&layout(&one, &fonts).unwrap()).width()
        };
        let all = bounds_of(&paths);
        assert!((all.width() - line(0)).abs() < 1e-6);
        // the middle line stretched: it reaches further right than alone
        let middle: Vec<Path> = paths
            .iter()
            .filter(|p| {
                let b = p.bounds();
                b.max_y < -5.0 && b.min_y > -25.0
            })
            .cloned()
            .collect();
        assert!(bounds_of(&middle).max_x > line(1) + 5.0);
    }

    #[test]
    fn styles_of_single_line_fonts() {
        let fonts = Fonts::builtin();
        let plain = layout(&spec("H", DEFAULT_FONT), &fonts).unwrap();
        let mut s = spec("H", DEFAULT_FONT);
        s.bold = true;
        assert_eq!(layout(&s, &fonts).unwrap().len(), plain.len() * 2);
        s.bold = false;
        s.italic = true;
        let it = bounds_of(&layout(&s, &fonts).unwrap());
        assert!(it.max_x > bounds_of(&plain).max_x + 1.0);
        s.italic = false;
        s.underline = true;
        s.strikethrough = true;
        let deco = layout(&s, &fonts).unwrap();
        assert_eq!(deco.len(), plain.len() + 2);
        assert!(!deco.last().unwrap().closed);
        assert!(bounds_of(&deco).min_y < -1.0);
        // the outline of a single-line text goes around its strokes
        let mut o = spec("I", DEFAULT_FONT);
        o.outline_mm = Some(1.0);
        let with = layout(&o, &fonts).unwrap();
        assert!(with.iter().any(|p| p.closed));
        assert!(bounds_of(&with).min_y < -0.9);
    }

    #[test]
    fn outline_fonts_weld_stencil_and_outline() {
        let fonts = testfont::fonts();
        let o = layout(&spec("O", testfont::FAMILY), &fonts).unwrap();
        // an O is an outer contour and a hole
        assert_eq!(o.len(), 2);
        assert!(o.iter().all(|p| p.closed));
        let b = bounds_of(&o);
        assert!((b.max_y - 10.0).abs() < 1e-6, "{b:?}");
        // two overlapping I's welded are one shape
        let mut s = spec("II", testfont::FAMILY);
        s.letter_spacing_mm = -8.0;
        assert_eq!(layout(&s, &fonts).unwrap().len(), 2);
        s.weld = true;
        assert_eq!(layout(&s, &fonts).unwrap().len(), 1);
        // the stencil cuts the O in two pieces, no hole left
        let mut st = spec("O", testfont::FAMILY);
        st.stencil_mm = Some(1.0);
        let pieces = layout(&st, &fonts).unwrap();
        assert_eq!(pieces.len(), 2, "{pieces:?}");
        for p in &pieces {
            assert!(boolean::signed_area(&p.to_polyline(0.01)) > 0.0);
        }
        // an outline around: the letters and a larger contour
        let mut ol = spec("I", testfont::FAMILY);
        ol.outline_mm = Some(1.0);
        let with = layout(&ol, &fonts).unwrap();
        assert_eq!(with.len(), 2);
        assert!((bounds_of(&with).max_y - 11.0).abs() < 0.05);
        // drawn bold grows the letters; underline is welded with them
        let mut bold = spec("I", testfont::FAMILY);
        bold.bold = true;
        assert!(bounds_of(&layout(&bold, &fonts).unwrap()).max_y > 10.2);
        let mut under = spec("I", testfont::FAMILY);
        under.underline = true;
        let u = layout(&under, &fonts).unwrap();
        assert_eq!(u.len(), 2, "the I and the band under it");
        assert!(bounds_of(&u).min_y < -1.0);
        // touching letters and underline are welded: a band through the I
        let mut low = TextSpec::new("I", 10.0, testfont::FAMILY);
        low.strikethrough = true;
        assert_eq!(layout(&low, &fonts).unwrap().len(), 1);
    }

    #[test]
    fn text_on_a_path_and_on_an_arc() {
        let fonts = Fonts::builtin();
        let vertical = Path::polyline(&[Point::ZERO, Point::new(0.0, 100.0)], false);
        let mut s = spec("HHH", DEFAULT_FONT);
        s.path = Some(TextPath {
            object: None,
            path: vertical.clone(),
            offset_mm: 10.0,
            distance_mm: 0.0,
            flip: false,
        });
        let b = bounds_of(&layout(&s, &fonts).unwrap());
        // going up, the letters stand to the left of the path
        assert!(
            b.max_x < 0.5 && b.min_x > -10.5 && b.min_y > 9.0 && b.max_y < 60.0,
            "{b:?}"
        );
        // flipped: the other side, from the top down
        s.path.as_mut().unwrap().flip = true;
        let f = bounds_of(&layout(&s, &fonts).unwrap());
        assert!(f.min_x > -0.5 && f.max_x < 10.5 && f.max_y < 90.5, "{f:?}");
        // distance moves the baseline away; centre and justify use the length
        s.path.as_mut().unwrap().flip = false;
        s.path.as_mut().unwrap().distance_mm = 5.0;
        assert!(bounds_of(&layout(&s, &fonts).unwrap()).max_x < -4.5);
        s.path.as_mut().unwrap().distance_mm = 0.0;
        s.path.as_mut().unwrap().offset_mm = 0.0;
        s.align = TextAlign::Center;
        let c = bounds_of(&layout(&s, &fonts).unwrap());
        assert!(((c.min_y + c.max_y) / 2.0 - 50.0).abs() < 1.5, "{c:?}");
        s.align = TextAlign::Justify;
        let j = bounds_of(&layout(&s, &fonts).unwrap());
        assert!(j.max_y > 95.0 && j.min_y < 3.0, "{j:?}");
        s.underline = true;
        assert!(layout(&s, &fonts).is_ok());
        // a path without length: a straight text
        s.path.as_mut().unwrap().path = Path::new(Point::ZERO);
        assert!(layout(&s, &fonts).is_ok());

        let mut a = spec("HELLO", DEFAULT_FONT);
        a.arc_radius_mm = Some(30.0);
        let top = bounds_of(&layout(&a, &fonts).unwrap());
        // centred on the top of the circle, bending down
        assert!(
            (top.min_x + top.max_x).abs() < 2.0 && top.max_y < 10.5 && top.min_y < -1.0,
            "{top:?}"
        );
        a.arc_radius_mm = Some(-30.0);
        let bottom = bounds_of(&layout(&a, &fonts).unwrap());
        assert!(
            (bottom.min_x + bottom.max_x).abs() < 2.0 && bottom.min_y > -0.5,
            "{bottom:?}"
        );
    }

    type Mutation = Box<dyn Fn(&mut TextSpec)>;

    #[test]
    fn invalid_values() {
        let fonts = Fonts::builtin();
        let base = spec("x", DEFAULT_FONT);
        let bad: Vec<Mutation> = vec![
            Box::new(|s| s.height_mm = 0.0),
            Box::new(|s| s.line_spacing = f64::NAN),
            Box::new(|s| s.width_scale = -1.0),
            Box::new(|s| s.letter_spacing_mm = f64::INFINITY),
            Box::new(|s| s.outline_mm = Some(0.0)),
            Box::new(|s| s.stencil_mm = Some(-1.0)),
            Box::new(|s| s.arc_radius_mm = Some(0.0)),
            Box::new(|s| {
                s.path = Some(TextPath {
                    object: None,
                    path: Path::new(Point::ZERO),
                    offset_mm: f64::NAN,
                    distance_mm: 0.0,
                    flip: false,
                });
            }),
            Box::new(|s| s.text = "x".repeat(MAX_CHARS + 1)),
            Box::new(|s| s.font = "Nope".into()),
        ];
        for f in bad {
            let mut s = base.clone();
            f(&mut s);
            assert!(layout(&s, &fonts).is_err(), "{s:?}");
        }
        assert!(layout(&spec("", DEFAULT_FONT), &fonts).unwrap().is_empty());
        assert!(!inside(
            &[Point::ZERO, Point::new(1.0, 0.0), Point::new(0.0, 1.0)],
            Point::new(2.0, 2.0)
        ));
    }
}
