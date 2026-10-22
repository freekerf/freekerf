//! The geometry of an SVG as LaserGRBL computes it (`SvgConverter/GCodeFromSVG.cs`,
//! from GRBL-Plotter): the same traversal (per container: rect, circle, ellipse,
//! line, polyline, polygon, text, image, then paths, then groups), the same units,
//! viewBox and transform handling, the same single precision where C# uses `float`,
//! Bézier flattening and elliptical arcs as cubic Béziers. Instead of G-code it
//! records paths of lines and counterclockwise arcs in millimetres; the planner
//! writes them back as LaserGRBL does.
//!
//! LaserGRBL bugs fixed here (see `doc/divergences.md`): compact numbers such as
//! `-4.5.5` (DIV-031), a `path` without `d` (DIV-032), `polyline`/`polygon` points
//! (DIV-034), `skewX`/`skewY` and a missing `)` in transforms (DIV-036).

use std::collections::HashMap;

use freekerf_document::{Color, ObjectKind, TextPath, TextSpec, UnsupportedReason};
use freekerf_geometry::{Matrix, Path, Point, Segment, bezier};
use roxmltree::Node;

use super::text::{self, StartOffset, SvgText};
use super::{SvgItem, SvgOptions, color};
use crate::ImportError;
use crate::netparse::parse_double;

const IN2PX: f32 = 96.0;
const MM2PX: f32 = 96.0 / 25.4;
const CM2PX: f32 = 96.0 / 2.54;
const PT2PX: f32 = 96.0 / 72.0;
const PC2PX: f32 = 12.0 * 96.0 / 72.0;
const EM2PX: f32 = 150.0;
const GROUP_LEVELS: usize = 10;
const BEZIER_SEGMENTS: u32 = 12;

/// `ConvertToPixel`: a length in px, from a unit suffix (the first of mm, cm, in, pt,
/// pc, em, % found after the first character), 0 when the number cannot be read.
/// Note LaserGRBL removes `"em "` (with a space) only, so `0.1em` reads as 0.
#[must_use]
pub fn to_pixel(s: &str, ext: f32) -> f32 {
    let pos = |unit: &str| s.find(unit).is_some_and(|i| i > 0);
    let mut percent = false;
    let factor = if pos("mm") {
        MM2PX
    } else if pos("cm") {
        CM2PX
    } else if pos("in") {
        IN2PX
    } else if pos("pt") {
        PT2PX
    } else if pos("pc") {
        PC2PX
    } else if pos("em") {
        EM2PX
    } else {
        percent = pos("%");
        1.0
    };
    let t = s
        .replace("pt", "")
        .replace("pc", "")
        .replace("mm", "")
        .replace("cm", "")
        .replace("in", "")
        .replace("em ", "")
        .replace('%', "")
        .replace("px", "");
    if t.is_empty() {
        return 0.0;
    }
    #[allow(clippy::cast_possible_truncation)]
    match parse_double(&t) {
        Some(v) if percent => v as f32 * ext / 100.0,
        Some(v) => v as f32 * factor,
        None => 0.0,
    }
}

/// The numbers of path data, as LaserGRBL's argument regex finds them, except that a
/// second `.` starts a new number (`-4.5.5` is `-4.5` and `.5`, DIV-031).
fn path_numbers(s: &str) -> Vec<&str> {
    let b = s.as_bytes();
    let digits = |mut i: usize| {
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        i
    };
    let mut out = Vec::new();
    let mut p = 0;
    while p < b.len() {
        let start = p;
        let signed = usize::from(b[p] == b'-');
        // (\-|)\d+(\.\d+|)((E|e)(\-|\+|)\d+|)
        let d = digits(start + signed);
        if d > start + signed {
            let mut end = d;
            if end + 1 < b.len() && b[end] == b'.' && b[end + 1].is_ascii_digit() {
                end = digits(end + 1);
            }
            if end < b.len() && (b[end] == b'e' || b[end] == b'E') {
                let mut e = end + 1;
                if e < b.len() && (b[e] == b'-' || b[e] == b'+') {
                    e += 1;
                }
                let ed = digits(e);
                if ed > e {
                    end = ed;
                }
            }
            out.push(&s[start..end]);
            p = end;
            continue;
        }
        // (\-|)\.\d+
        let dot = start + signed;
        if dot + 1 < b.len() && b[dot] == b'.' && b[dot + 1].is_ascii_digit() {
            let end = digits(dot + 1);
            out.push(&s[start..end]);
            p = end;
            continue;
        }
        p += 1;
    }
    out
}

/// `Regex.Split(d, "(?=[A-Za-z-[e]])")` without empty tokens: a token starts at every
/// ASCII letter except `e`.
fn path_tokens(d: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, c) in d.char_indices() {
        if i > start && c.is_ascii_alphabetic() && c != 'e' {
            out.push(&d[start..i]);
            start = i;
        }
    }
    if start < d.len() {
        out.push(&d[start..]);
    }
    out.into_iter().filter(|t| !t.is_empty()).collect()
}

/// `GetTextBetween`: the arguments after `prefix` up to the first character that is
/// not part of a number list (the whole rest when none: DIV-036 fixes the lost last
/// character of LaserGRBL).
fn text_between<'a>(source: &'a str, prefix: &str) -> &'a str {
    let start = source.find(prefix).map_or(0, |i| i + prefix.len());
    let rest = &source[start.min(source.len())..];
    let end = rest
        .find(|c: char| !(c.is_numeric() || matches!(c, '.' | ',' | ' ' | '-' | 'e')))
        .unwrap_or(rest.len());
    &rest[..end]
}

/// Splits like `coord.Split(',')`, or on spaces when there is no comma.
fn split_args(coord: &str) -> Vec<&str> {
    if coord.contains(',') {
        coord.split(',').collect()
    } else {
        coord.split(' ').collect()
    }
}

/// `ParseTransform` for one function (`translate(…)`, `scale(…)`, `rotate(…)`,
/// `matrix(…)`, and, unlike LaserGRBL, `skewX(…)`/`skewY(…)`).
fn parse_transform_fn(t: &str) -> Matrix {
    let mut m = Matrix::IDENTITY;
    if t.contains("translate") {
        let a = split_args(text_between(t, "translate("));
        m.offset_x = f64::from(to_pixel(a[0], 1.0));
        if a.len() > 1 {
            m.offset_y = f64::from(to_pixel(a[1].trim_end_matches(')'), 1.0));
        }
    }
    if t.contains("scale") {
        let coord = text_between(t, "scale(");
        let a = split_args(coord);
        m.m11 = f64::from(to_pixel(a[0], 1.0));
        if a.len() > 1 {
            m.m22 = f64::from(to_pixel(a[1], 1.0));
        } else {
            m.m11 = f64::from(to_pixel(coord, 1.0));
            m.m22 = f64::from(to_pixel(coord, 1.0));
        }
    }
    if t.contains("rotate") {
        let a = split_args(text_between(t, "rotate("));
        let angle = to_pixel(a[0], 1.0);
        let (px, py) = if a.len() == 3 {
            (to_pixel(a[1], 1.0), to_pixel(a[2], 1.0))
        } else {
            (0.0, 0.0)
        };
        m.rotate_at(f64::from(angle), f64::from(px), f64::from(py));
    }
    if t.contains("matrix") {
        let a = split_args(text_between(t, "matrix("));
        let v = |i: usize| f64::from(a.get(i).map_or(0.0, |s| to_pixel(s, 1.0)));
        m = Matrix::new(v(0), v(1), v(2), v(3), v(4), v(5));
    }
    for (name, x) in [("skewX(", true), ("skewY(", false)] {
        if t.contains(name) {
            let a = split_args(text_between(t, name));
            let tan = (f64::from(to_pixel(a[0], 1.0)) * std::f64::consts::PI / 180.0).tan();
            let mut s = Matrix::IDENTITY;
            if x {
                s.m21 = tan;
            } else {
                s.m12 = tan;
            }
            m = Matrix::multiply(&s, &m);
        }
    }
    m
}

/// The transform attribute as LaserGRBL reads it: the text is cut before every
/// `trans`, `sca`, `rot`, `mat` (and `skew`) and each piece is one function; later
/// functions are applied first (`Matrix.Multiply(piece, final)`).
fn parse_transform_attr(transform: &str) -> Matrix {
    let mut words: Vec<String> = vec![transform.to_owned()];
    for sep in ["trans", "sca", "rot", "mat", "skew"] {
        words = words
            .iter()
            .flat_map(|w| w.split(sep).map(str::to_owned).collect::<Vec<_>>())
            .collect();
    }
    // the pieces keep their order; rebuild each with the separator it lost
    let mut final_m = Matrix::IDENTITY;
    let mut rest = transform;
    for w in words.into_iter().filter(|w| !w.is_empty()) {
        let Some(at) = rest.find(&w) else {
            continue;
        };
        let before = &rest[..at];
        rest = &rest[at + w.len()..];
        let full = if w.starts_with("late") && before.ends_with("trans") {
            format!("trans{w}")
        } else if w.starts_with("le") && before.ends_with("sca") {
            format!("sca{w}")
        } else if w.starts_with("ate") && before.ends_with("rot") {
            format!("rot{w}")
        } else if w.starts_with("rix") && before.ends_with("mat") {
            format!("mat{w}")
        } else if (w.starts_with('X') || w.starts_with('Y')) && before.ends_with("skew") {
            format!("skew{w}")
        } else {
            continue;
        };
        final_m = Matrix::multiply(&parse_transform_fn(&full), &final_m);
    }
    final_m
}

/// Calculates the vector angle (`CalculateVectorAngle`).
fn vector_angle(ux: f64, uy: f64, vx: f64, vy: f64) -> f64 {
    let ta = uy.atan2(ux);
    let tb = vy.atan2(vx);
    if tb >= ta {
        tb - ta
    } else {
        std::f64::consts::PI * 2.0 - (ta - tb)
    }
}

/// The converter state (`GCodeFromSVG` fields used for geometry).
pub(super) struct Converter<'o> {
    opts: &'o SvgOptions,
    ns: Option<String>,
    matrix_group: [Matrix; GROUP_LEVELS],
    matrix_element: Matrix,
    scaled_error: f64,
    svg_width_px: f32,
    svg_height_px: f32,
    current_x: f32,
    current_y: f32,
    first: Option<(f32, f32)>,
    last_x: f32,
    last_y: f32,
    cx_mirror: f32,
    cy_mirror: f32,
    start_path: bool,
    start_sub_path: bool,
    // what is being recorded
    color: Color,
    paths: Vec<Path>,
    current: Option<Path>,
    items: Vec<SvgItem>,
    last_end: HashMap<Color, Point>,
    /// Item of each element with an `id` (the paths texts follow).
    ids: HashMap<String, usize>,
    /// Texts to lay out once every path is known: item, text, matrix at the text.
    texts: Vec<(usize, SvgText, Matrix)>,
}

impl<'o> Converter<'o> {
    pub(super) fn new(opts: &'o SvgOptions) -> Self {
        Self {
            opts,
            ns: None,
            matrix_group: [Matrix::IDENTITY; GROUP_LEVELS],
            matrix_element: Matrix::IDENTITY,
            scaled_error: 1.0,
            svg_width_px: 0.0,
            svg_height_px: 0.0,
            current_x: 0.0,
            current_y: 0.0,
            first: None,
            last_x: 0.0,
            last_y: 0.0,
            cx_mirror: 0.0,
            cy_mirror: 0.0,
            start_path: true,
            start_sub_path: true,
            color: Color::BLACK,
            paths: Vec::new(),
            current: None,
            items: Vec::new(),
            last_end: HashMap::new(),
            ids: HashMap::new(),
            texts: Vec::new(),
        }
    }

    pub(super) fn run(mut self, root: Node<'_, '_>) -> Result<Vec<SvgItem>, ImportError> {
        self.globals(root)?;
        self.basic_elements(root, 1)?;
        self.path_elements(root, 1)?;
        self.groups(root, 1)?;
        self.lay_out_texts(root);
        Ok(self.items)
    }

    /// Remembers the item an element with an `id` made.
    fn record_id(&mut self, e: Node<'_, '_>, before: usize) {
        if self.items.len() > before
            && let Some(id) = e.attribute("id")
        {
            self.ids.entry(id.to_owned()).or_insert(before);
        }
    }

    /// A `text` element: an item laid out at the end ([`Self::lay_out_texts`]).
    fn text_item(&mut self, e: Node<'_, '_>) {
        let Some(t) = text::read(e) else { return };
        self.end_item();
        self.texts.push((self.items.len(), t, self.matrix_element));
        self.items
            .push(SvgItem::new(self.color, ObjectKind::Shape { paths: Vec::new() }));
    }

    /// The first path of the element `id` that no item made (a `path` in `defs`).
    fn unconverted_path(&self, root: Node<'_, '_>, id: &str) -> Option<Path> {
        let e = root
            .descendants()
            .find(|n| self.is(*n, "path") && n.attribute("id") == Some(id))?;
        let mut sub = Converter::new(self.opts);
        sub.ns.clone_from(&self.ns);
        sub.matrix_group = self.matrix_group;
        sub.matrix_element = self.matrix_group[0];
        sub.scaled_error = self.scaled_error;
        sub.path_element(e, 1).ok()?;
        sub.items.into_iter().find_map(|i| match i.kind {
            ObjectKind::Shape { paths } => paths.into_iter().next(),
            _ => None,
        })
    }

    /// Lays the texts out with the import's fonts: straight from their position
    /// (the object's transform takes them from mm at the origin to the drawing),
    /// or on the path a `textPath` names. A text that cannot be laid out stays a
    /// "not supported" item.
    fn lay_out_texts(&mut self, root: Node<'_, '_>) {
        let Some(fonts) = self.opts.fonts.clone() else {
            return;
        };
        for (index, t, m) in std::mem::take(&mut self.texts) {
            let mm_per_unit = m.determinant().abs().sqrt();
            let family = t
                .family
                .clone()
                .filter(|f| fonts.face(f, false, false).is_ok())
                .unwrap_or_else(|| freekerf_text::DEFAULT_FONT.to_owned());
            let Ok(face) = fonts.face(&family, t.bold, t.italic) else {
                continue;
            };
            let mut spec = TextSpec::new(
                t.lines.join("\n"),
                t.size * mm_per_unit * face.face.cap_height_per_em(),
                family,
            );
            spec.bold = t.bold;
            spec.italic = t.italic;
            spec.underline = t.underline;
            spec.strikethrough = t.strikethrough;
            spec.align = t.align;
            spec.letter_spacing_mm = t.letter_spacing * mm_per_unit;
            let mut transform = Matrix::IDENTITY;
            let mut link = None;
            if let Some(on) = &t.path {
                link = self.ids.get(&on.id).copied();
                let path = link
                    .and_then(|i| match &self.items[i].kind {
                        ObjectKind::Shape { paths } => paths.first().cloned(),
                        _ => None,
                    })
                    .or_else(|| self.unconverted_path(root, &on.id));
                if let Some(path) = path {
                    let offset_mm = match on.start {
                        StartOffset::Length(l) => l * mm_per_unit,
                        StartOffset::Percent(p) => p / 100.0 * path.length(),
                    };
                    spec.path = Some(TextPath {
                        object: None,
                        path,
                        offset_mm,
                        distance_mm: 0.0,
                        flip: on.flip,
                    });
                } else {
                    link = None;
                }
            }
            if spec.path.is_none() {
                // local mm, Y up -> user units, Y down -> the drawing's mm
                transform = Matrix::new(1.0 / mm_per_unit, 0.0, 0.0, -1.0 / mm_per_unit, t.x, t.y);
                transform.append(&m);
            }
            let item = &mut self.items[index];
            match freekerf_text::layout(&spec, &fonts) {
                Ok(paths) if mm_per_unit > 0.0 => {
                    item.kind = ObjectKind::Shape {
                        paths: paths.iter().map(|p| p.transformed(&transform, 0.01)).collect(),
                    };
                    item.text = Some(spec);
                    item.transform = transform;
                    item.link = link;
                }
                _ => {
                    item.kind = ObjectKind::Unsupported {
                        element: "text".into(),
                        reason: UnsupportedReason::NotConverted,
                    };
                }
            }
        }
    }

    fn is(&self, node: Node<'_, '_>, name: &str) -> bool {
        node.is_element()
            && node.tag_name().name() == name
            && node.tag_name().namespace() == self.ns.as_deref()
    }

    // ------------------------------------------------------------ geometry recording

    fn translate_xy(&self, p: Point) -> Point {
        self.matrix_element.transform(p)
    }

    fn translate_ij(&self, i: f32, j: f32) -> Point {
        let m = &self.matrix_element;
        let (i, j) = (f64::from(i), f64::from(j));
        Point::new(i * m.m11 + j * m.m21, i * m.m12 + j * m.m22)
    }

    fn finish_path(&mut self) {
        if let Some(p) = self.current.take() {
            self.paths.push(p);
        }
    }

    /// `gcodeStartPath`.
    fn start_path_at(&mut self, x: f32, y: f32) {
        self.finish_path();
        let coord = self.translate_xy(Point::new(f64::from(x), f64::from(y)));
        self.current = Some(Path::new(coord));
    }

    /// The path being drawn. Drawing without a moveto continues from where the laser
    /// is (the end of the previous element of the same colour); when there is none,
    /// the path starts at `first` and `true` is returned (DIV-107).
    fn current_or_continuation(&mut self, first: Point) -> (&mut Path, bool) {
        let mut started_here = false;
        if self.current.is_none() {
            let start = self
                .paths
                .last()
                .map(Path::end)
                .or_else(|| self.last_end.get(&self.color).copied());
            started_here = start.is_none();
            let mut p = Path::new(start.unwrap_or(first));
            p.from_current = !started_here;
            self.current = Some(p);
        }
        (self.current.get_or_insert_with(|| Path::new(first)), started_here)
    }

    /// `gcodeMoveTo` (a point already in user units).
    fn move_to_point(&mut self, p: Point) {
        let coord = self.translate_xy(p);
        let (path, started_here) = self.current_or_continuation(coord);
        if !started_here {
            path.segments.push(Segment::line(coord));
        }
    }

    fn move_to(&mut self, x: f32, y: f32) {
        self.move_to_point(Point::new(f64::from(x), f64::from(y)));
    }

    /// `gcodeArcToCCW`: a counterclockwise arc (always `G3`, as LaserGRBL writes it).
    fn arc_to_ccw(&mut self, x: f32, y: f32, i: f32, j: f32) {
        let end = self.translate_xy(Point::new(f64::from(x), f64::from(y)));
        let ij = self.translate_ij(i, j);
        let (path, started_here) = self.current_or_continuation(end);
        if !started_here {
            let start = path.end();
            path.segments.push(Segment::arc(end, start + ij, true));
        }
    }

    /// `gcodeStopPath` / `gcodePenUp`.
    fn stop_path(&mut self) {
        self.finish_path();
    }

    fn comment_item(&mut self, element: &str, reason: UnsupportedReason) {
        self.end_item();
        self.items.push(SvgItem::new(
            self.color,
            ObjectKind::Unsupported {
                element: element.to_owned(),
                reason,
            },
        ));
    }

    fn end_item(&mut self) {
        self.finish_path();
        if !self.paths.is_empty() {
            let paths = std::mem::take(&mut self.paths);
            if let Some(end) = paths.last().map(Path::end) {
                self.last_end.insert(self.color, end);
            }
            self.items
                .push(SvgItem::new(self.color, ObjectKind::Shape { paths }));
        }
    }

    // ------------------------------------------------------------ traversal

    /// `parseGlobals`: viewBox, width, height, scaling, user offset.
    fn globals(&mut self, root: Node<'_, '_>) -> Result<(), ImportError> {
        let mut tmp = Matrix::IDENTITY;
        self.svg_width_px = 0.0;
        self.svg_height_px = 0.0;
        let (mut vb_off_x, mut vb_off_y, mut vb_width, mut vb_height) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);

        // the default namespace declared on the root (LaserGRBL reads the `xmlns` attribute)
        self.ns = root
            .namespaces()
            .find(|ns| ns.name().is_none())
            .map(|ns| ns.uri().to_owned());
        if let Some(vb) = root.attribute("viewBox") {
            // Regex.Replace(viewbox, @"\s+", " ").Split(' ')
            let mut text = String::with_capacity(vb.len());
            let mut in_space = false;
            for c in vb.chars() {
                if c.is_whitespace() {
                    if !in_space {
                        text.push(' ');
                    }
                    in_space = true;
                } else {
                    text.push(c);
                    in_space = false;
                }
            }
            let split: Vec<&str> = text.split(' ').collect();
            if split.len() < 4 {
                return Err(ImportError::Invalid(format!(
                    "viewBox `{vb}` needs four numbers separated by spaces"
                )));
            }
            vb_off_x = -to_pixel(split[0], 1.0);
            vb_off_y = -to_pixel(split[1], 1.0);
            vb_width = to_pixel(split[2], 1.0);
            vb_height = to_pixel(split[3].trim_end_matches(')'), 1.0);
            tmp.m11 = 1.0;
            tmp.m22 = -1.0;
            tmp.offset_y = f64::from(vb_height);
        }
        let scale: f32 = 1.0 / MM2PX;
        if let Some(w) = root.attribute("width") {
            self.svg_width_px = to_pixel(w, 1.0);
            tmp.m11 = f64::from(scale);
            if vb_width > 0.0 {
                let e = scale * self.svg_width_px / vb_width;
                self.scaled_error = f64::from(e);
                tmp.m11 = f64::from(e);
                tmp.offset_x = f64::from((vb_off_x * self.svg_width_px / vb_width) * scale);
            }
        }
        if let Some(h) = root.attribute("height") {
            self.svg_height_px = to_pixel(h, 1.0);
            tmp.m22 = f64::from(-scale);
            tmp.offset_y = f64::from(scale * self.svg_height_px);
            if vb_height > 0.0 {
                tmp.m22 = f64::from(-scale * self.svg_height_px / vb_height);
                tmp.offset_y =
                    f64::from((-vb_off_y * self.svg_height_px / vb_height + self.svg_height_px) * scale);
            }
        }
        let new_width = self.svg_width_px.max(vb_width);
        let new_height = self.svg_height_px.max(vb_height);
        if new_width > 0.0
            && new_height > 0.0
            && let Some(max_size) = self.opts.scale_to_mm
        {
            let gcode_scale = max_size / new_width.max(new_height);
            tmp.scale(f64::from(gcode_scale), f64::from(gcode_scale));
            tmp.scale(f64::from(MM2PX), f64::from(MM2PX));
        }
        tmp.offset_x += f64::from(self.opts.user_offset.0);
        tmp.offset_y += f64::from(self.opts.user_offset.1);
        self.matrix_group = [tmp; GROUP_LEVELS];
        self.matrix_element = tmp;
        Ok(())
    }

    /// `parseTransform`.
    fn transform(&mut self, node: Node<'_, '_>, is_group: bool, level: usize) -> Result<(), ImportError> {
        if level >= GROUP_LEVELS {
            return Err(ImportError::Invalid(format!(
                "groups nested deeper than {} levels",
                GROUP_LEVELS - 1
            )));
        }
        let final_m = node
            .attribute("transform")
            .map_or(Matrix::IDENTITY, parse_transform_attr);
        if is_group {
            if level > 0 {
                let g = Matrix::multiply(&final_m, &self.matrix_group[level - 1]);
                for m in &mut self.matrix_group[level..] {
                    *m = g;
                }
            } else {
                self.matrix_group[level] = final_m;
            }
            self.matrix_element = self.matrix_group[level];
        } else {
            self.matrix_element = Matrix::multiply(&final_m, &self.matrix_group[level]);
        }
        Ok(())
    }

    /// `parseGroup`.
    fn groups(&mut self, node: Node<'_, '_>, level: usize) -> Result<(), ImportError> {
        let groups: Vec<_> = node.children().filter(|c| self.is(*c, "g")).collect();
        for g in groups {
            self.transform(g, true, level)?;
            self.basic_elements(g, level)?;
            self.path_elements(g, level)?;
            self.groups(g, level + 1)?;
        }
        Ok(())
    }

    fn attr(node: Node<'_, '_>, name: &str, ext: f32) -> f32 {
        node.attribute(name).map_or(0.0, |v| to_pixel(v, ext))
    }

    /// `parseBasicElements`.
    #[allow(clippy::too_many_lines, clippy::float_cmp)]
    fn basic_elements(&mut self, node: Node<'_, '_>, level: usize) -> Result<(), ImportError> {
        for form in [
            "rect", "circle", "ellipse", "line", "polyline", "polygon", "text", "image",
        ] {
            let elements: Vec<_> = node.children().filter(|c| self.is(*c, form)).collect();
            for e in elements {
                let before = self.items.len();
                self.color = color::resolve(e);
                let old = self.matrix_element;
                self.transform(e, false, level)?;
                let x = Self::attr(e, "x", 1.0);
                let y = Self::attr(e, "y", 1.0);
                let x1 = Self::attr(e, "x1", 1.0);
                let y1 = Self::attr(e, "y1", 1.0);
                let x2 = Self::attr(e, "x2", 1.0);
                let y2 = Self::attr(e, "y2", 1.0);
                let width = Self::attr(e, "width", self.svg_width_px);
                let height = Self::attr(e, "height", self.svg_height_px);
                let mut rx = Self::attr(e, "rx", 1.0);
                let mut ry = Self::attr(e, "ry", 1.0);
                let cx = Self::attr(e, "cx", 1.0);
                let cy = Self::attr(e, "cy", 1.0);
                let r = Self::attr(e, "r", 1.0);
                match form {
                    "rect" => {
                        if ry == 0.0 {
                            ry = rx;
                        } else if rx == 0.0 {
                            rx = ry;
                        } else if rx != ry {
                            rx = rx.min(ry);
                            ry = rx;
                        }
                        self.start_path_at(x + rx, y + height);
                        self.move_to(x + width - rx, y + height);
                        if rx > 0.0 {
                            self.arc_to_ccw(x + width, y + height - ry, 0.0, -ry);
                        }
                        self.move_to(x + width, y + ry);
                        if rx > 0.0 {
                            self.arc_to_ccw(x + width - rx, y, -rx, 0.0);
                        }
                        self.move_to(x + rx, y);
                        if rx > 0.0 {
                            self.arc_to_ccw(x, y + ry, 0.0, ry);
                        }
                        self.move_to(x, y + height - ry);
                        if rx > 0.0 {
                            self.arc_to_ccw(x + rx, y + height, rx, 0.0);
                            self.move_to(x + rx, y + height);
                        }
                        self.stop_path();
                    }
                    "circle" => {
                        self.start_path_at(cx + r, cy);
                        self.arc_to_ccw(cx + r, cy, -r, 0.0);
                        self.stop_path();
                    }
                    "ellipse" => {
                        self.start_path_at(cx + rx, cy);
                        self.calc_arc(cx + rx, cy, rx, ry, 0.0, 1.0, 1.0, cx - rx, cy);
                        self.calc_arc(cx - rx, cy, rx, ry, 0.0, 1.0, 1.0, cx + rx, cy);
                        self.stop_path();
                    }
                    "line" => {
                        self.start_path_at(x1, y1);
                        self.move_to(x2, y2);
                        self.stop_path();
                    }
                    "polyline" | "polygon" => {
                        // the coordinate pairs of `points` (SVG syntax: numbers separated by
                        // white space and/or one comma, DIV-034)
                        let nums: Vec<f32> = e
                            .attribute("points")
                            .unwrap_or("")
                            .split(|c: char| c == ',' || c.is_whitespace())
                            .filter(|t| !t.is_empty())
                            .map(|t| to_pixel(t, 1.0))
                            .collect();
                        if let [px, py, ..] = nums[..] {
                            self.start_path_at(px, py);
                            for pair in nums[2..].chunks_exact(2) {
                                self.move_to(pair[0], pair[1]);
                            }
                            if form == "polygon" {
                                self.move_to(px, py);
                            }
                            self.stop_path();
                        }
                    }
                    "text" if self.opts.fonts.is_some() => self.text_item(e),
                    _ => self.comment_item(form, UnsupportedReason::NotConverted),
                }
                self.end_item();
                self.record_id(e, before);
                self.matrix_element = old;
            }
        }
        Ok(())
    }

    /// `parsePath`.
    fn path_elements(&mut self, node: Node<'_, '_>, level: usize) -> Result<(), ImportError> {
        let elements: Vec<_> = node.children().filter(|c| self.is(*c, "path")).collect();
        for e in elements {
            let before = self.items.len();
            self.path_element(e, level)?;
            self.record_id(e, before);
        }
        Ok(())
    }

    /// One `path` element.
    fn path_element(&mut self, e: Node<'_, '_>, level: usize) -> Result<(), ImportError> {
        // a path without `d` makes LaserGRBL throw and lose the whole file (DIV-032)
        let Some(d) = e.attribute("d") else {
            return Ok(());
        };
        self.color = color::resolve(e);
        self.current_x = 0.0;
        self.current_y = 0.0;
        self.first = None;
        self.start_path = true;
        self.start_sub_path = true;
        self.last_x = 0.0;
        self.last_y = 0.0;
        let old = self.matrix_element;
        self.transform(e, false, level)?;
        for token in path_tokens(d) {
            self.path_command(token);
        }
        self.end_item();
        self.matrix_element = old;
        Ok(())
    }

    fn bezier(&mut self, points: [Point; 4]) {
        let b = if self.opts.legacy_bezier {
            bezier::uniform(&points, BEZIER_SEGMENTS)
        } else {
            bezier::flatten(&points, 0.2 / self.scaled_error, bezier::MAX_SUBDIVISIONS)
        };
        for p in b.into_iter().skip(1) {
            #[allow(clippy::cast_possible_truncation)]
            self.move_to(p.x as f32, p.y as f32);
        }
    }

    /// `parsePathCommand`.
    #[allow(clippy::too_many_lines)]
    fn path_command(&mut self, token: &str) {
        let Some(command) = token.chars().next() else {
            return;
        };
        let cmd = command.to_ascii_uppercase();
        let absolute = cmd == command;
        let args: Vec<f32> = path_numbers(&token[command.len_utf8()..])
            .into_iter()
            .map(|s| to_pixel(s, 1.0))
            .collect();
        let pt = |x: f32, y: f32| Point::new(f64::from(x), f64::from(y));
        match cmd {
            'M' => {
                for (i, pair) in args.chunks_exact(2).enumerate() {
                    if absolute || self.start_path {
                        self.current_x = pair[0];
                        self.current_y = pair[1];
                    } else {
                        self.current_x = pair[0] + self.last_x;
                        self.current_y = pair[1] + self.last_y;
                    }
                    if self.start_sub_path {
                        self.start_path_at(self.current_x, self.current_y);
                        self.first = Some((self.current_x, self.current_y));
                        self.start_path = false;
                        self.start_sub_path = false;
                    } else if i == 0 {
                        self.start_path_at(self.current_x, self.current_y);
                    } else {
                        self.move_to(self.current_x, self.current_y);
                    }
                    if self.first.is_none() {
                        self.first = Some((self.current_x, self.current_y));
                    }
                    self.last_x = self.current_x;
                    self.last_y = self.current_y;
                }
                self.cx_mirror = self.current_x;
                self.cy_mirror = self.current_y;
            }
            'Z' => {
                let (fx, fy) = self.first.unwrap_or((self.current_x, self.current_y));
                self.move_to(fx, fy);
                if let Some(p) = &mut self.current {
                    p.closed = true;
                }
                self.last_x = fx;
                self.last_y = fy;
                self.first = None;
                self.start_sub_path = true;
                self.stop_path();
            }
            'L' => {
                for pair in args.chunks_exact(2) {
                    if absolute {
                        self.current_x = pair[0];
                        self.current_y = pair[1];
                    } else {
                        self.current_x = self.last_x + pair[0];
                        self.current_y = self.last_y + pair[1];
                    }
                    self.move_to(self.current_x, self.current_y);
                    self.last_x = self.current_x;
                    self.last_y = self.current_y;
                    self.cx_mirror = self.current_x;
                    self.cy_mirror = self.current_y;
                }
                self.start_sub_path = true;
            }
            'H' | 'V' => {
                for &a in &args {
                    if cmd == 'H' {
                        self.current_x = if absolute { a } else { self.last_x + a };
                        self.current_y = self.last_y;
                    } else {
                        self.current_x = self.last_x;
                        self.current_y = if absolute { a } else { self.last_y + a };
                    }
                    self.move_to(self.current_x, self.current_y);
                    self.last_x = self.current_x;
                    self.last_y = self.current_y;
                    self.cx_mirror = self.current_x;
                    self.cy_mirror = self.current_y;
                }
                self.start_sub_path = true;
            }
            'A' => {
                for a in args.chunks_exact(7) {
                    let (nx, ny) = if absolute {
                        (a[5], a[6])
                    } else {
                        (a[5] + self.last_x, a[6] + self.last_y)
                    };
                    self.calc_arc(self.last_x, self.last_y, a[0], a[1], a[2], a[3], a[4], nx, ny);
                    self.last_x = nx;
                    self.last_y = ny;
                }
                self.start_sub_path = true;
            }
            'C' => {
                let mut rep = 0;
                while rep < args.len() {
                    if rep + 5 < args.len() {
                        let a = &args[rep..rep + 6];
                        let (cx1, cy1, cx2, cy2, cx3, cy3) = if absolute {
                            (a[0], a[1], a[2], a[3], a[4], a[5])
                        } else {
                            let (lx, ly) = (self.last_x, self.last_y);
                            (lx + a[0], ly + a[1], lx + a[2], ly + a[3], lx + a[4], ly + a[5])
                        };
                        self.bezier([
                            pt(self.last_x, self.last_y),
                            pt(cx1, cy1),
                            pt(cx2, cy2),
                            pt(cx3, cy3),
                        ]);
                        self.cx_mirror = cx3 - (cx2 - cx3);
                        self.cy_mirror = cy3 - (cy2 - cy3);
                        self.last_x = cx3;
                        self.last_y = cy3;
                    } else {
                        self.comment_item("path", UnsupportedReason::MissingArgument);
                    }
                    rep += 6;
                }
                self.start_sub_path = true;
            }
            'S' => {
                for a in args.chunks_exact(4) {
                    let (cx2, cy2, cx3, cy3) = if absolute {
                        (a[0], a[1], a[2], a[3])
                    } else {
                        let (lx, ly) = (self.last_x, self.last_y);
                        (lx + a[0], ly + a[1], lx + a[2], ly + a[3])
                    };
                    self.bezier([
                        pt(self.last_x, self.last_y),
                        pt(self.cx_mirror, self.cy_mirror),
                        pt(cx2, cy2),
                        pt(cx3, cy3),
                    ]);
                    self.cx_mirror = cx3 - (cx2 - cx3);
                    self.cy_mirror = cy3 - (cy2 - cy3);
                    self.last_x = cx3;
                    self.last_y = cy3;
                }
                self.start_sub_path = true;
            }
            'Q' | 'T' => {
                let step = if cmd == 'Q' { 4 } else { 2 };
                for a in args.chunks_exact(step) {
                    let (lx, ly) = (self.last_x, self.last_y);
                    let (cx2, cy2, cx3, cy3) = if cmd == 'Q' {
                        if absolute {
                            (a[0], a[1], a[2], a[3])
                        } else {
                            (lx + a[0], ly + a[1], lx + a[2], ly + a[3])
                        }
                    } else {
                        let (x3, y3) = if absolute {
                            (a[0], a[1])
                        } else {
                            (lx + a[0], ly + a[1])
                        };
                        (self.cx_mirror, self.cy_mirror, x3, y3)
                    };
                    // shorten the control points to 2/3 to use a cubic
                    let qpx1 = (cx2 - lx) * 2.0 / 3.0 + lx;
                    let qpy1 = (cy2 - ly) * 2.0 / 3.0 + ly;
                    let qpx2 = (cx2 - cx3) * 2.0 / 3.0 + cx3;
                    let qpy2 = (cy2 - cy3) * 2.0 / 3.0 + cy3;
                    if cmd == 'Q' {
                        self.cx_mirror = cx3 - (cx2 - cx3);
                        self.cy_mirror = cy3 - (cy2 - cy3);
                    } else {
                        self.cx_mirror = cx3;
                        self.cy_mirror = cy3;
                    }
                    self.last_x = cx3;
                    self.last_y = cy3;
                    self.bezier([pt(lx, ly), pt(qpx1, qpy1), pt(qpx2, qpy2), pt(cx3, cy3)]);
                }
                self.start_sub_path = true;
            }
            _ => {}
        }
    }

    /// `calcArc`: an elliptical arc as cubic Béziers (from vvvv/SVG's `SvgArcSegment`).
    /// As in LaserGRBL, an arc with a zero radius draws nothing (the SVG specification
    /// says a straight line; DIV-035, kept): with both radii zero it returns at once,
    /// with one its segment count is NaN.
    #[allow(clippy::too_many_arguments, clippy::similar_names, clippy::float_cmp)]
    fn calc_arc(
        &mut self,
        sx: f32,
        sy: f32,
        radius_x: f32,
        radius_y: f32,
        angle: f32,
        size: f32,
        sweep: f32,
        ex: f32,
        ey: f32,
    ) {
        if radius_x == 0.0 || radius_y == 0.0 {
            return;
        }
        // StartX - EndX and StartX + EndX are float operations in C#
        let (dx, dy) = (f64::from(sx - ex), f64::from(sy - ey));
        let (mx, my) = (f64::from(sx + ex), f64::from(sy + ey));
        let sin_phi = (f64::from(angle) * std::f64::consts::PI / 180.0).sin();
        let cos_phi = (f64::from(angle) * std::f64::consts::PI / 180.0).cos();
        let x1dash = cos_phi * dx / 2.0 + sin_phi * dy / 2.0;
        let y1dash = -sin_phi * dx / 2.0 + cos_phi * dy / 2.0;
        // RadiusX * RadiusX ... are float products in C#
        let rx2 = f64::from(radius_x * radius_x);
        let ry2 = f64::from(radius_y * radius_y);
        let rxry = f64::from(radius_x * radius_x * radius_y * radius_y);
        let numerator = rxry - rx2 * y1dash * y1dash - ry2 * x1dash * x1dash;
        let mut rx = radius_x;
        let mut ry = radius_y;
        let root = if numerator < 0.0 {
            #[allow(clippy::cast_possible_truncation)]
            let s = (1.0 - numerator / rxry).sqrt() as f32;
            rx *= s;
            ry *= s;
            0.0
        } else {
            let sign = if (size == 1.0 && sweep == 1.0) || (size == 0.0 && sweep == 0.0) {
                -1.0
            } else {
                1.0
            };
            sign * (numerator / (rx2 * y1dash * y1dash + ry2 * x1dash * x1dash)).sqrt()
        };
        let (rx, ry) = (f64::from(rx), f64::from(ry));
        let cxdash = root * rx * y1dash / ry;
        let cydash = -root * ry * x1dash / rx;
        let cx = cos_phi * cxdash - sin_phi * cydash + mx / 2.0;
        let cy = sin_phi * cxdash + cos_phi * cydash + my / 2.0;
        let mut theta1 = vector_angle(1.0, 0.0, (x1dash - cxdash) / rx, (y1dash - cydash) / ry);
        let mut dtheta = vector_angle(
            (x1dash - cxdash) / rx,
            (y1dash - cydash) / ry,
            (-x1dash - cxdash) / rx,
            (-y1dash - cydash) / ry,
        );
        if sweep == 0.0 && dtheta > 0.0 {
            dtheta -= 2.0 * std::f64::consts::PI;
        } else if sweep == 1.0 && dtheta < 0.0 {
            dtheta += 2.0 * std::f64::consts::PI;
        }
        #[allow(clippy::cast_possible_truncation)]
        let segments = (dtheta / (std::f64::consts::PI / 2.0)).abs().ceil() as i32;
        let delta = dtheta / f64::from(segments);
        let t = 8.0 / 3.0 * (delta / 4.0).sin() * (delta / 4.0).sin() / (delta / 2.0).sin();
        let mut start_x = f64::from(sx);
        let mut start_y = f64::from(sy);
        for _ in 0..segments {
            let (cos1, sin1) = (theta1.cos(), theta1.sin());
            let theta2 = theta1 + delta;
            let (cos2, sin2) = (theta2.cos(), theta2.sin());
            let end_x = cos_phi * rx * cos2 - sin_phi * ry * sin2 + cx;
            let end_y = sin_phi * rx * cos2 + cos_phi * ry * sin2 + cy;
            let dx1 = t * (-cos_phi * rx * sin1 - sin_phi * ry * cos1);
            let dy1 = t * (-sin_phi * rx * sin1 + cos_phi * ry * cos1);
            let dxe = t * (cos_phi * rx * sin2 + sin_phi * ry * cos2);
            let dye = t * (sin_phi * rx * sin2 - cos_phi * ry * cos2);
            let points = [
                Point::new(start_x, start_y),
                Point::new(start_x + dx1, start_y + dy1),
                Point::new(end_x + dxe, end_y + dye),
                Point::new(end_x, end_y),
            ];
            let b = if self.opts.legacy_bezier {
                bezier::uniform(&points, BEZIER_SEGMENTS)
            } else {
                bezier::flatten(&points, 0.2 / self.scaled_error, bezier::MAX_SUBDIVISIONS)
            };
            for p in b.into_iter().skip(1) {
                self.move_to_point(p);
            }
            theta1 = theta2;
            #[allow(clippy::cast_possible_truncation)]
            {
                start_x = f64::from(end_x as f32);
                start_y = f64::from(end_y as f32);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_and_failures() {
        assert!((to_pixel("10mm", 1.0) - 10.0 * MM2PX).abs() < 1e-4);
        assert_eq!(to_pixel("1in", 1.0), 96.0);
        assert_eq!(to_pixel("72pt", 1.0), 96.0);
        assert_eq!(to_pixel("1pc", 1.0), 16.0);
        assert_eq!(to_pixel("2cm", 1.0), 2.0 * CM2PX);
        assert_eq!(to_pixel("0.1em", 1.0), 0.0); // LaserGRBL removes "em " only
        assert_eq!(to_pixel("50%", 200.0), 100.0);
        assert_eq!(to_pixel("abc%", 200.0), 0.0);
        assert_eq!(to_pixel("1.2.3", 1.0), 0.0);
        assert_eq!(to_pixel("", 1.0), 0.0);
        assert_eq!(to_pixel("12px", 1.0), 12.0);
        assert_eq!(to_pixel("mm5", 1.0), 5.0); // a unit at index 0 is removed but not applied
    }

    #[test]
    fn numbers_and_tokens() {
        assert_eq!(
            path_numbers("10 -4.5.5-1e3,.5 x 1.5e-2"),
            vec!["10", "-4.5", ".5", "-1e3", ".5", "1.5e-2"]
        );
        assert_eq!(path_numbers("1."), vec!["1"]);
        assert_eq!(path_numbers("-.5e2"), vec!["-.5", "2"]);
        assert_eq!(path_tokens(" M1 2L3 4e5z"), vec![" ", "M1 2", "L3 4e5", "z"]);
        assert_eq!(path_tokens("M1E5"), vec!["M1", "E5"]);
    }

    #[test]
    fn transforms_as_laser_grbl_reads_them() {
        let m = parse_transform_attr("translate(50 50) rotate(45)");
        let p = m.transform(Point::new(1.0, 0.0));
        assert!((p.x - (50.0 + 0.5f64.sqrt())).abs() < 1e-6 && (p.y - (50.0 + 0.5f64.sqrt())).abs() < 1e-6);
        let s = parse_transform_attr("scale(2)");
        assert_eq!((s.m11, s.m22), (2.0, 2.0));
        let s = parse_transform_attr("scale(1 3)");
        assert_eq!((s.m11, s.m22), (1.0, 3.0));
        let t = parse_transform_attr("translate(5,5");
        assert_eq!((t.offset_x, t.offset_y), (5.0, 5.0));
        let k = parse_transform_attr("skewX(45)");
        assert!((k.m21 - 1.0).abs() < 1e-12);
        let k = parse_transform_attr("skewY(45) translate(1)");
        assert!((k.m12 - 1.0).abs() < 1e-12 && k.offset_x != 0.0);
        let mx = parse_transform_attr("matrix(2 0 0 1 20 80)");
        assert_eq!(mx, Matrix::new(2.0, 0.0, 0.0, 1.0, 20.0, 80.0));
        let r = parse_transform_attr("rotate(90, 1, 1)");
        let q = r.transform(Point::new(2.0, 1.0));
        assert!((q.x - 1.0).abs() < 1e-6 && (q.y - 2.0).abs() < 1e-6);
        assert_eq!(parse_transform_attr("bogus(1)"), Matrix::IDENTITY);
        assert_eq!(text_between("x(1 2)", "x("), "1 2");
    }
}
