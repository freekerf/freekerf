//! Objects of a document.

use freekerf_geometry::{Matrix, Path, Point, Rect};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{DocumentError, ImageObject, LayerId};

/// Tolerance, mm, of arcs that a transform turns into lines.
const TRANSFORM_TOLERANCE_MM: f64 = 0.01;

fn is_identity(m: &Matrix) -> bool {
    *m == Matrix::IDENTITY
}

/// Identifier of an object inside a document (never reused).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct ObjectId(pub u64);

/// The file format an object was imported from: how LaserGRBL wrote its G-code
/// differs per source (pen comments in SVG, none in DXF), and the planner keeps
/// that when asked for LaserGRBL-compatible output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceFormat {
    /// SVG drawing.
    Svg,
    /// DXF drawing.
    Dxf,
    /// Raster image.
    Image,
    /// Drawn in FreeKerf.
    Native,
}

/// Where an object came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Source {
    /// Format.
    pub format: SourceFormat,
    /// File name (no directory), empty when unknown.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub file: String,
}

/// Why an element of an imported file was not converted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UnsupportedReason {
    /// The element kind is not converted (SVG `text`, `image`).
    NotConverted,
    /// A `polyline`/`polygon` whose points have no commas.
    MissingComma,
    /// A path command without all its arguments (a curve missing a coordinate).
    MissingArgument,
}

/// What an object is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ObjectKind {
    /// Vector paths in millimetres (Y up), in drawing order.
    Shape {
        /// Paths.
        paths: Vec<Path>,
    },
    /// A raster image.
    Image(Box<ImageObject>),
    /// An element of an imported file that was not converted; kept so the user
    /// sees it (and LaserGRBL-compatible output writes its comment).
    Unsupported {
        /// Element name (`text`, `image`, `polygon`...).
        element: String,
        /// Reason.
        reason: UnsupportedReason,
    },
}

/// Font family of text written before fonts could be chosen (`.fkproj` version 3
/// and older): the Hershey font of LaserGRBL's test generators.
pub const LEGACY_FONT: &str = "LaserGRBL Hershey";

fn legacy_font() -> String {
    LEGACY_FONT.to_owned()
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(b: &bool) -> bool {
    !*b
}

#[allow(clippy::trivially_copy_pass_by_ref, clippy::float_cmp)]
fn is_one(v: &f64) -> bool {
    *v == 1.0
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

const fn one() -> f64 {
    1.0
}

/// How the lines of a text line up.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    /// Lines start at the origin.
    #[default]
    Left,
    /// Lines are centred on the origin.
    Center,
    /// Lines end at the origin.
    Right,
    /// Lines (but the last) fill the width of the longest one; on a path, the
    /// text fills the path.
    Justify,
}

impl TextAlign {
    /// `true` for the default (left).
    #[must_use]
    pub fn is_left(&self) -> bool {
        *self == Self::Left
    }
}

/// The baseline a text follows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TextPath {
    /// The object whose first path is the baseline: the text follows it when it
    /// changes. `None` once that object is gone (the text keeps the last copy).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<ObjectId>,
    /// The baseline as it was at the last layout, mm (document coordinates).
    pub path: Path,
    /// Where the text starts along the path, mm (with the alignment).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub offset_mm: f64,
    /// Distance of the baseline from the path, mm (positive: to the left of the
    /// path's direction, where the letters stand).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub distance_mm: f64,
    /// Follow the path backwards (the text goes to its other side).
    #[serde(default, skip_serializing_if = "is_false")]
    pub flip: bool,
}

/// The text an object was made from (editable: the editor lays it out again when
/// the text, its font or its style change, at the origin, then applies the
/// object's `transform`; a text on a linked path is laid out along that path).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TextSpec {
    /// The text; `\n` starts a new line.
    pub text: String,
    /// Height of the capital letters, mm.
    pub height_mm: f64,
    /// Font family (a built-in single-line font or an installed font).
    #[serde(default = "legacy_font")]
    pub font: String,
    /// Bold (the family's bold face, else drawn thicker).
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    /// Italic (the family's italic face, else slanted).
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    /// Underlined.
    #[serde(default, skip_serializing_if = "is_false")]
    pub underline: bool,
    /// Struck through.
    #[serde(default, skip_serializing_if = "is_false")]
    pub strikethrough: bool,
    /// Alignment of the lines.
    #[serde(default, skip_serializing_if = "TextAlign::is_left")]
    pub align: TextAlign,
    /// Distance between baselines, times the font's line height.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub line_spacing: f64,
    /// Space added between letters, mm (negative: tighter).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub letter_spacing_mm: f64,
    /// Horizontal scale of the letters (1 = the font's width).
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub width_scale: f64,
    /// Outline fonts: the letters are merged into one shape where they overlap
    /// (cutting script fonts).
    #[serde(default, skip_serializing_if = "is_false")]
    pub weld: bool,
    /// An outline this far around the letters is added, mm (negative: inside).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline_mm: Option<f64>,
    /// Outline fonts: bridges this wide hold the inner parts of the letters (the
    /// middle of an O), mm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stencil_mm: Option<f64>,
    /// The text is bent along a circle of this radius, mm, centred under it
    /// (negative: above it, the text reads along the bottom).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arc_radius_mm: Option<f64>,
    /// The text follows a path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<TextPath>,
}

impl TextSpec {
    /// A text in `font`, `height_mm` high, with the default style.
    #[must_use]
    pub fn new(text: impl Into<String>, height_mm: f64, font: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            height_mm,
            font: font.into(),
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            align: TextAlign::Left,
            line_spacing: 1.0,
            letter_spacing_mm: 0.0,
            width_scale: 1.0,
            weld: false,
            outline_mm: None,
            stencil_mm: None,
            arc_radius_mm: None,
            path: None,
        }
    }

    /// The object whose path this text follows.
    #[must_use]
    pub fn linked_path(&self) -> Option<ObjectId> {
        self.path.as_ref().and_then(|p| p.object)
    }
}

/// An object on a layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Object {
    /// Identifier.
    pub id: ObjectId,
    /// Layer it belongs to.
    pub layer: LayerId,
    /// Display name (element id, file name...).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Origin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    /// The text of a text object (its paths are the strokes); none otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<TextSpec>,
    /// Every transform applied since the object was created or imported. The
    /// geometry already includes it: the editor reads the rotation from it, and a
    /// text's strokes are written at the origin and then transformed by it.
    #[serde(default, skip_serializing_if = "is_identity")]
    pub transform: Matrix,
    /// Content.
    #[serde(flatten)]
    pub kind: ObjectKind,
}

impl Object {
    /// Bounds in mm (empty for unsupported elements).
    #[must_use]
    pub fn bounds(&self) -> Rect {
        let mut r = Rect::EMPTY;
        match &self.kind {
            ObjectKind::Shape { paths } => {
                for p in paths {
                    r.union(&p.bounds());
                }
            }
            ObjectKind::Image(img) => {
                r.add(img.position);
                r.add(Point::new(
                    img.position.x + img.size_mm[0],
                    img.position.y + img.size_mm[1],
                ));
            }
            ObjectKind::Unsupported { .. } => {}
        }
        r
    }

    /// Applies a transform to the geometry and adds it to `transform`. Images take
    /// only translations and positive scalings (they stay axis-aligned).
    ///
    /// # Errors
    /// A rotation, skew or mirror of an image; nothing changes then.
    pub fn apply_transform(&mut self, m: &Matrix) -> Result<(), DocumentError> {
        match &mut self.kind {
            ObjectKind::Shape { paths } => {
                for p in paths.iter_mut() {
                    *p = p.transformed(m, TRANSFORM_TOLERANCE_MM);
                }
            }
            ObjectKind::Image(img) => {
                if m.m12 != 0.0 || m.m21 != 0.0 || m.m11 <= 0.0 || m.m22 <= 0.0 {
                    return Err(DocumentError::Invalid(
                        "images can only be moved and scaled".into(),
                    ));
                }
                img.position = m.transform(img.position);
                img.size_mm = [img.size_mm[0] * m.m11, img.size_mm[1] * m.m22];
            }
            ObjectKind::Unsupported { .. } => {}
        }
        self.transform.append(m);
        Ok(())
    }

    /// Writes a text object's strokes again from `paths` given at the origin
    /// (the new text or height), through the object's `transform`.
    pub fn set_text_paths(&mut self, spec: TextSpec, paths: &[Path]) {
        let m = self.transform;
        self.kind = ObjectKind::Shape {
            paths: paths
                .iter()
                .map(|p| p.transformed(&m, TRANSFORM_TOLERANCE_MM))
                .collect(),
        };
        self.text = Some(spec);
    }

    /// Moves the object.
    pub fn translate(&mut self, dx: f64, dy: f64) {
        match &mut self.kind {
            ObjectKind::Shape { paths } => {
                for p in paths {
                    p.translate(dx, dy);
                }
            }
            ObjectKind::Image(img) => {
                img.position.x += dx;
                img.position.y += dy;
            }
            ObjectKind::Unsupported { .. } => {}
        }
        self.transform.translate(dx, dy);
    }
}

/// A closed rectangle path, corners `(x, y)` and `(x + width, y + height)`.
#[must_use]
pub fn rect_path(x: f64, y: f64, width: f64, height: f64) -> Path {
    let pts = [
        Point::new(x, y),
        Point::new(x + width, y),
        Point::new(x + width, y + height),
        Point::new(x, y + height),
    ];
    Path::polyline(&pts, true)
}

/// A closed ellipse path inscribed in the rectangle, four cubic Béziers
/// (counterclockwise from the right).
#[must_use]
pub fn ellipse_path(x: f64, y: f64, width: f64, height: f64) -> Path {
    use freekerf_geometry::Segment;
    // the usual cubic approximation of a quarter circle
    const K: f64 = 0.552_284_749_830_793_4;
    let (rx, ry) = (width / 2.0, height / 2.0);
    let (cx, cy) = (x + rx, y + ry);
    let (kx, ky) = (rx * K, ry * K);
    let mut p = Path::new(Point::new(cx + rx, cy));
    let quarter = |c1: (f64, f64), c2: (f64, f64), end: (f64, f64)| Segment::Cubic {
        c1: Point::new(c1.0, c1.1),
        c2: Point::new(c2.0, c2.1),
        end: Point::new(end.0, end.1),
    };
    p.segments = vec![
        quarter((cx + rx, cy + ky), (cx + kx, cy + ry), (cx, cy + ry)),
        quarter((cx - kx, cy + ry), (cx - rx, cy + ky), (cx - rx, cy)),
        quarter((cx - rx, cy - ky), (cx - kx, cy - ry), (cx, cy - ry)),
        quarter((cx + kx, cy - ry), (cx + rx, cy - ky), (cx + rx, cy)),
    ];
    p.closed = true;
    p
}
