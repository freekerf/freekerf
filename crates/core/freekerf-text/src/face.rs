//! Font faces: single-line ([`StrokeFont`]) or outline (TrueType / OpenType, read
//! with `skrifa` and shaped with `harfrust`), behind one interface: metrics and
//! the glyphs of a line of text, in font units, Y up.

use std::sync::Arc;

use freekerf_geometry::{Path, Point, Segment};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, GlyphId, MetadataProvider};

use crate::TextError;
use crate::svgfont::StrokeFont;

/// Vertical metrics of a face, font units (Y up, descent negative).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Metrics {
    pub units_per_em: f64,
    pub ascent: f64,
    pub descent: f64,
    pub line_gap: f64,
    pub cap_height: f64,
    /// The capitals' height as drawn (differs from `cap_height` in LaserGRBL's
    /// Hershey only); letters from a fallback font match it.
    pub drawn_cap_height: f64,
    pub x_height: f64,
    pub underline_position: f64,
    pub underline_thickness: f64,
    pub strikeout_position: f64,
    pub strikeout_thickness: f64,
}

impl Metrics {
    /// Distance between baselines at line spacing 1.
    pub(crate) fn line_height(&self) -> f64 {
        self.ascent - self.descent + self.line_gap
    }
}

/// A glyph of a shaped line: its outline at the pen position and how far it moves
/// the pen, font units.
#[derive(Debug, Clone, Default)]
pub(crate) struct ShapedGlyph {
    pub paths: Vec<Path>,
    pub x_advance: f64,
    pub x_offset: f64,
    pub y_offset: f64,
    /// White space (justified lines stretch these).
    pub space: bool,
}

/// A TrueType or OpenType font (or one face of a collection).
#[derive(Clone)]
pub struct OutlineFont {
    data: Arc<[u8]>,
    index: u32,
    /// The same font for the shaper (`read-fonts`' model, shared by clones).
    shaping: harfrust::Font,
    metrics: Metrics,
}

impl std::fmt::Debug for OutlineFont {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OutlineFont")
            .field("bytes", &self.data.len())
            .field("index", &self.index)
            .field("metrics", &self.metrics)
            .finish_non_exhaustive()
    }
}

#[derive(Default)]
struct Outline {
    paths: Vec<Path>,
    current: Option<Path>,
}

impl Outline {
    fn path(&mut self) -> &mut Path {
        self.current.get_or_insert_with(|| Path::new(Point::ZERO))
    }

    fn finish(&mut self) {
        if let Some(p) = self.current.take()
            && !p.segments.is_empty()
        {
            self.paths.push(p);
        }
    }
}

fn pt(x: f32, y: f32) -> Point {
    Point::new(f64::from(x), f64::from(y))
}

impl OutlinePen for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.finish();
        self.current = Some(Path::new(pt(x, y)));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.path().segments.push(Segment::line(pt(x, y)));
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let p = self.path();
        let (p0, c, end) = (p.end(), pt(cx0, cy0), pt(x, y));
        p.segments.push(Segment::Cubic {
            c1: p0 + (c - p0) * (2.0 / 3.0),
            c2: end + (c - end) * (2.0 / 3.0),
            end,
        });
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.path().segments.push(Segment::Cubic {
            c1: pt(cx0, cy0),
            c2: pt(cx1, cy1),
            end: pt(x, y),
        });
    }

    fn close(&mut self) {
        if let Some(p) = &mut self.current {
            if p.end() != p.start {
                let start = p.start;
                p.segments.push(Segment::line(start));
            }
            p.closed = true;
        }
        self.finish();
    }
}

/// The outline of a glyph in font units (empty for `.notdef` or a glyph without one).
fn outline(font: &FontRef<'_>, glyph: GlyphId) -> Vec<Path> {
    let mut pen = Outline::default();
    if glyph != GlyphId::NOTDEF
        && let Some(g) = font.outline_glyphs().get(glyph)
    {
        let settings = DrawSettings::unhinted(Size::unscaled(), LocationRef::default());
        if g.draw(settings, &mut pen).is_err() {
            return Vec::new();
        }
    }
    pen.finish();
    pen.paths
}

impl OutlineFont {
    /// Reads face `index` of a font file.
    ///
    /// # Errors
    /// Not a font this reader understands.
    pub fn new(data: Arc<[u8]>, index: u32) -> Result<Self, TextError> {
        let font = FontRef::from_index(&data, index).map_err(|e| TextError::Font(e.to_string()))?;
        let m = font.metrics(Size::unscaled(), LocationRef::default());
        let upem = f64::from(m.units_per_em);
        let measured = |c: char| {
            let glyph = font.charmap().map(c)?;
            let mut r = freekerf_geometry::Rect::EMPTY;
            for p in outline(&font, glyph) {
                r.union(&p.bounds());
            }
            Some(r.max_y).filter(|h| h.is_finite() && *h > 0.0)
        };
        let positive = |v: Option<f32>| v.map(f64::from).filter(|h| *h > 0.0);
        let cap_height = positive(m.cap_height)
            .or_else(|| measured('H'))
            .unwrap_or(upem * 0.7);
        let x_height = positive(m.x_height)
            .or_else(|| measured('x'))
            .unwrap_or(cap_height * 0.65);
        // a decoration's offset is to its top: the layout wants its middle
        let decoration = |d: Option<skrifa::metrics::Decoration>, middle: f64| {
            d.filter(|d| d.thickness > 0.0)
                .map_or((middle, upem * 0.05), |d| {
                    let t = f64::from(d.thickness);
                    (f64::from(d.offset) - t / 2.0, t)
                })
        };
        let (underline_position, underline_thickness) = decoration(m.underline, -upem * 0.1);
        let (strikeout_position, strikeout_thickness) = decoration(m.strikeout, x_height / 2.0);
        let metrics = Metrics {
            units_per_em: upem,
            ascent: f64::from(m.ascent),
            descent: f64::from(m.descent),
            line_gap: f64::from(m.leading),
            cap_height,
            drawn_cap_height: cap_height,
            x_height,
            underline_position,
            underline_thickness,
            strikeout_position,
            strikeout_thickness,
        };
        let shaping = harfrust::Font::new(data.to_vec(), index)
            .ok_or_else(|| TextError::Font("not a font the shaper can read".into()))?;
        Ok(Self {
            data,
            index,
            shaping,
            metrics,
        })
    }

    pub(crate) const fn metrics(&self) -> &Metrics {
        &self.metrics
    }

    /// Whether the font has a glyph for `c`.
    #[must_use]
    pub fn covers(&self, c: char) -> bool {
        FontRef::from_index(&self.data, self.index).is_ok_and(|f| f.charmap().map(c).is_some())
    }

    /// The glyphs of one line: kerning, ligatures, marks and right-to-left scripts
    /// as the font's OpenType tables say.
    pub(crate) fn shape(&self, text: &str) -> Vec<ShapedGlyph> {
        let Ok(font) = FontRef::from_index(&self.data, self.index) else {
            return Vec::new();
        };
        let mut buffer = harfrust::Buffer::new();
        buffer.push_str(text);
        buffer.guess_segment_properties();
        let shaper = harfrust::ShaperFont::new(&self.shaping);
        if harfrust::shape(&shaper, &mut buffer, harfrust::ShapeOptions::new()).is_err() {
            return Vec::new();
        }
        buffer
            .glyph_infos()
            .iter()
            .zip(buffer.glyph_positions())
            .map(|(info, pos)| {
                let space = usize::try_from(info.cluster)
                    .ok()
                    .and_then(|i| text.get(i..))
                    .and_then(|s| s.chars().next())
                    .is_some_and(char::is_whitespace);
                ShapedGlyph {
                    paths: outline(&font, GlyphId::new(info.glyph_id)),
                    x_advance: f64::from(pos.x_advance),
                    x_offset: f64::from(pos.x_offset),
                    y_offset: f64::from(pos.y_offset),
                    space,
                }
            })
            .collect()
    }
}

/// A font face ready to lay text out.
#[derive(Debug, Clone)]
pub enum Face {
    /// Open strokes, drawn once.
    Stroke(Arc<StrokeFont>),
    /// Closed outlines.
    Outline(Arc<OutlineFont>),
}

impl Face {
    pub(crate) fn metrics(&self) -> &Metrics {
        match self {
            Self::Stroke(f) => f.metrics(),
            Self::Outline(f) => f.metrics(),
        }
    }

    /// Height of the capitals over the em (an SVG `font-size` is the em).
    #[must_use]
    pub fn cap_height_per_em(&self) -> f64 {
        let m = self.metrics();
        m.cap_height / m.units_per_em
    }

    /// Single-line (open strokes).
    #[must_use]
    pub const fn single_line(&self) -> bool {
        matches!(self, Self::Stroke(_))
    }

    pub(crate) fn shape(&self, text: &str) -> Vec<ShapedGlyph> {
        match self {
            Self::Stroke(f) => f.shape(text),
            Self::Outline(f) => f.shape(text),
        }
    }

    /// Whether the face has a glyph for `c`.
    #[must_use]
    pub fn covers(&self, c: char) -> bool {
        match self {
            Self::Stroke(f) => f.covers(c),
            Self::Outline(f) => f.covers(c),
        }
    }
}
