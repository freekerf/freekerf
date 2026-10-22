//! The document, selection, history and view actions (PLAN §9.1). They are
//! registered into the host's registry ([`register_session_actions`]), next to the
//! machine actions, so the UI, scripts and agents reach both the same way.
//! Reading or writing files is the host's job: these actions take bytes and text.

use std::collections::BTreeSet;
use std::sync::{Mutex, MutexGuard};

use base64::Engine;
use freekerf_document::{
    Bytes, Color, Document, ImageData, ImageObject, Layer, LayerId, LayerMode, Object, ObjectId, ObjectKind,
    RasterSettings, Source, SourceFormat, TextAlign, TextPath, TextSpec, ellipse_path, rect_path,
};
use freekerf_geometry::hatch::FillPattern;
use freekerf_geometry::{Matrix, Path, Point, Rect};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::session::{Mode, Session, Snapshot, ViewState, Viewport};
use crate::view::{Dialog, Panel, ToastLevel, Tool, ViewCommand};
use crate::{ActionError, NoParams, Registry, RegistryError, Tier};

/// A registry context that holds a session.
pub trait SessionHost: Send + Sync + 'static {
    /// The session.
    fn session(&self) -> &Mutex<Session>;
}

fn lock<C: SessionHost>(c: &C) -> MutexGuard<'_, Session> {
    c.session()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn failed(e: impl std::fmt::Display) -> ActionError {
    ActionError::failed(e.to_string())
}

/// Applies a view command to the session's view; returns a new toast's id.
///
/// # Errors
/// An invalid viewport or an empty toast.
pub fn view_cmd<C: SessionHost>(c: &C, cmd: ViewCommand) -> Result<Option<u64>, ActionError> {
    let mut s = lock(c);
    let mut v = s.view().clone();
    let r = v
        .apply(cmd)
        .map_err(|e| ActionError::InvalidParams(e.to_string()))?;
    s.change_view(|view| *view = v);
    Ok(r)
}

fn view_unit<C: SessionHost>(c: &C, cmd: ViewCommand) -> Result<Changed, ActionError> {
    view_cmd(c, cmd).map(|_| Changed { changed: true })
}

/// Object ids given by a caller, or the selection when none.
fn targets(ids: Option<Vec<u64>>, s: &Session) -> BTreeSet<ObjectId> {
    ids.map_or_else(
        || s.selection().clone(),
        |v| v.into_iter().map(ObjectId).collect(),
    )
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct FkprojParams {
    /// The project file's text (`.fkproj`, JSON).
    text: String,
}

#[derive(Serialize, JsonSchema)]
struct FkprojText {
    /// The project as `.fkproj` text.
    text: String,
}

/// File formats of `document.import_data`.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ImportFormat {
    /// SVG drawing (colour layers).
    Svg,
    /// ASCII DXF drawing.
    Dxf,
    /// PNG, JPEG, BMP or GIF image.
    Image,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ImportParams {
    /// File format.
    format: ImportFormat,
    /// The file's bytes, base64 (or `text`).
    #[serde(default)]
    data: Option<String>,
    /// SVG or DXF: the file as text instead of `data` (for scripts that build a
    /// drawing).
    #[serde(default)]
    text: Option<String>,
    /// Name of the file (kept as the objects' source).
    #[serde(default)]
    name: String,
    /// Images: resolution in dots per inch (default 96); position is the origin.
    #[serde(default)]
    dpi: Option<f64>,
    /// Speed of new layers, mm/min (default 1000).
    #[serde(default)]
    speed_mm_min: Option<u32>,
    /// Power of new layers, `S` (default 1000).
    #[serde(default)]
    power: Option<u32>,
    /// Drawings: `keep` the colour layers (default), or `unify` everything on the
    /// current layer.
    #[serde(default)]
    colors: ImportColors,
}

/// Layers of an imported drawing.
#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ImportColors {
    /// One layer per colour (an existing layer of the same colour is reused).
    #[default]
    Keep,
    /// Everything on the current layer.
    Unify,
}

#[derive(Serialize, JsonSchema)]
struct Imported {
    /// Ids of the new objects.
    objects: Vec<u64>,
    /// Ids of the layers the import created.
    layers: Vec<u32>,
    /// Elements kept but not converted (SVG text, images...).
    unsupported: usize,
}

impl Imported {
    fn new(doc: &Document, before: &[LayerId], ids: Vec<ObjectId>) -> Self {
        Self {
            unsupported: ids
                .iter()
                .filter(|id| {
                    doc.object(**id)
                        .is_some_and(|o| matches!(o.kind, ObjectKind::Unsupported { .. }))
                })
                .count(),
            layers: doc
                .layers
                .iter()
                .filter(|l| !before.contains(&l.id))
                .map(|l| l.id.0)
                .collect(),
            objects: ids.into_iter().map(|i| i.0).collect(),
        }
    }
}

/// How `document.select` combines the ids with the current selection.
#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum SelectMode {
    /// Select exactly these.
    #[default]
    Replace,
    /// Add these.
    Add,
    /// Remove these.
    Remove,
    /// Add those not selected, remove those selected.
    Toggle,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SelectParams {
    /// Object ids.
    ids: Vec<u64>,
    /// Replace (default), add or remove.
    #[serde(default)]
    mode: SelectMode,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct IdsParams {
    /// Object ids (default: the selection).
    #[serde(default)]
    ids: Option<Vec<u64>>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TranslateParams {
    /// Distance along X, mm.
    dx: f64,
    /// Distance along Y, mm.
    dy: f64,
    /// Object ids (default: the selection).
    #[serde(default)]
    ids: Option<Vec<u64>>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct MoveToLayerParams {
    /// Layer id.
    layer: u32,
    /// Object ids (default: the selection).
    #[serde(default)]
    ids: Option<Vec<u64>>,
}

#[derive(Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
struct LayerFields {
    /// Name.
    #[serde(default)]
    name: Option<String>,
    /// `line`, `fill`, `fill_and_line`, `cut` or `ignore`.
    #[serde(default)]
    mode: Option<LayerMode>,
    /// Speed, mm/min.
    #[serde(default)]
    speed_mm_min: Option<u32>,
    /// `S` power.
    #[serde(default)]
    power: Option<u32>,
    /// Passes (at least 1).
    #[serde(default)]
    passes: Option<u32>,
    /// Shown and planned.
    #[serde(default)]
    visible: Option<bool>,
    /// Filling pattern.
    #[serde(default)]
    fill_pattern: Option<FillPattern>,
    /// Filling lines per mm.
    #[serde(default)]
    lines_per_mm: Option<f64>,
    /// Laser on command: `machine` (the machine's), `constant` (`M3`) or `dynamic` (`M4`).
    #[serde(default)]
    laser_mode: Option<freekerf_document::LaserMode>,
}

impl LayerFields {
    fn apply(self, l: &mut Layer) -> Result<(), ActionError> {
        if let Some(v) = self.name {
            l.name = v;
        }
        if let Some(v) = self.mode {
            l.mode = v;
        }
        if let Some(v) = self.speed_mm_min {
            l.speed_mm_min = v;
        }
        if let Some(v) = self.power {
            l.power = v;
        }
        if let Some(v) = self.passes {
            if v == 0 {
                return Err(ActionError::InvalidParams("passes must be at least 1".into()));
            }
            l.passes = v;
        }
        if let Some(v) = self.visible {
            l.visible = v;
        }
        if let Some(v) = self.laser_mode {
            l.laser_mode = v;
        }
        if let Some(v) = self.fill_pattern {
            l.fill.pattern = v;
        }
        if let Some(v) = self.lines_per_mm {
            if !(v > 0.0 && v.is_finite()) {
                return Err(ActionError::InvalidParams("lines_per_mm must be positive".into()));
            }
            l.fill.lines_per_mm = v;
        }
        Ok(())
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AddLayerParams {
    /// Colour, `#RRGGBB`.
    color: Color,
    /// Other fields (defaults: line, 1000 mm/min, power 1000, 1 pass).
    #[serde(flatten)]
    fields: LayerFields,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct UpdateLayerParams {
    /// Layer id.
    id: u32,
    /// Fields to change.
    #[serde(flatten)]
    fields: LayerFields,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct LayerIdParams {
    /// Layer id.
    id: u32,
}

#[derive(Serialize, JsonSchema)]
struct Changed {
    /// Whether anything changed.
    changed: bool,
}

#[derive(Serialize, JsonSchema)]
struct NewId {
    /// Id of what was created.
    id: u64,
}

#[derive(Serialize, JsonSchema)]
struct HistoryResult {
    /// Label of the step undone or redone; none when there was nothing to do.
    step: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ModeParams {
    /// `design` or `produce`.
    mode: Mode,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ToolParams {
    /// `select`, `rect`, `ellipse` or `text`.
    tool: Tool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PanelParams {
    /// `layers`, `object`, `machine` or `console`.
    name: Panel,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DialogParams {
    /// The dialog (`settings`, `shortcuts`, `recover`, `welcome`, `import`, `raster`, `about`...).
    name: Dialog,
}

#[derive(Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct OptionalDialogParams {
    /// The dialog (default: the one on top).
    #[serde(default)]
    name: Option<Dialog>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ToastParams {
    /// `info` (default), `ok`, `warning` or `error`.
    #[serde(default)]
    level: ToastLevel,
    /// i18n key (or plain text).
    message: String,
    /// Placeholders of the message.
    #[serde(default)]
    args: std::collections::BTreeMap<String, String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ToastIdParams {
    /// Id returned by `view.toast`.
    id: u64,
}

#[derive(Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct FocusParams {
    /// Element with the focus (`canvas`, `console`...), none when absent.
    #[serde(default)]
    target: Option<String>,
}

/// Shapes of `document.add_shape`.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ShapeKind {
    /// Rectangle.
    Rect,
    /// Ellipse inscribed in the rectangle.
    Ellipse,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AddShapeParams {
    /// `rect` or `ellipse`.
    shape: ShapeKind,
    /// Left, mm.
    x: f64,
    /// Bottom, mm (Y up).
    y: f64,
    /// Width, mm (positive).
    width: f64,
    /// Height, mm (positive).
    height: f64,
    /// Layer id (default: the current layer; a black layer when there is none).
    #[serde(default)]
    layer: Option<u32>,
}

/// A text action's parameters plus the style fields every text action takes
/// (all optional: unchanged, or the default of a new text), and `style`, which
/// applies them to a spec.
macro_rules! with_text_style {
    ($(#[$m:meta])* struct $name:ident { $($(#[$fm:meta])* $f:ident: $t:ty,)* }) => {
        $(#[$m])*
        struct $name {
            $($(#[$fm])* $f: $t,)*
            /// Font family (`fonts.list`; default for a new text: Relief SingleLine).
            #[serde(default)]
            font: Option<String>,
            /// Bold.
            #[serde(default)]
            bold: Option<bool>,
            /// Italic.
            #[serde(default)]
            italic: Option<bool>,
            /// Underlined.
            #[serde(default)]
            underline: Option<bool>,
            /// Struck through.
            #[serde(default)]
            strikethrough: Option<bool>,
            /// Alignment of the lines.
            #[serde(default)]
            align: Option<TextAlign>,
            /// Distance between baselines, times the font's line height (default 1).
            #[serde(default)]
            line_spacing: Option<f64>,
            /// Space added between letters, mm (default 0).
            #[serde(default)]
            letter_spacing_mm: Option<f64>,
            /// Horizontal scale of the letters (default 1).
            #[serde(default)]
            width_scale: Option<f64>,
            /// Outline fonts: merge overlapping letters into one shape.
            #[serde(default)]
            weld: Option<bool>,
            /// Add an outline this far around the letters, mm (negative: inside; 0: none).
            #[serde(default)]
            outline_mm: Option<f64>,
            /// Outline fonts: stencil bridges this wide, mm (0: none).
            #[serde(default)]
            stencil_mm: Option<f64>,
            /// Bend the text on a circle of this radius, mm (negative: along its
            /// bottom; 0: straight). Not for a text on a path.
            #[serde(default)]
            arc_radius_mm: Option<f64>,
        }

        impl $name {
            fn style(&self, s: &mut TextSpec) {
                let some = |v: f64| (v != 0.0).then_some(v);
                if let Some(v) = &self.font {
                    s.font.clone_from(v);
                }
                s.bold = self.bold.unwrap_or(s.bold);
                s.italic = self.italic.unwrap_or(s.italic);
                s.underline = self.underline.unwrap_or(s.underline);
                s.strikethrough = self.strikethrough.unwrap_or(s.strikethrough);
                s.align = self.align.unwrap_or(s.align);
                s.line_spacing = self.line_spacing.unwrap_or(s.line_spacing);
                s.letter_spacing_mm = self.letter_spacing_mm.unwrap_or(s.letter_spacing_mm);
                s.width_scale = self.width_scale.unwrap_or(s.width_scale);
                s.weld = self.weld.unwrap_or(s.weld);
                if let Some(v) = self.outline_mm {
                    s.outline_mm = some(v);
                }
                if let Some(v) = self.stencil_mm {
                    s.stencil_mm = some(v);
                }
                if let Some(v) = self.arc_radius_mm {
                    s.arc_radius_mm = some(v);
                }
            }
        }
    };
}

with_text_style! {
    #[derive(Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct AddTextParams {
        /// The text; `\n` starts a new line.
        text: String,
        /// Where the first baseline starts (left), is centred or ends (right), mm.
        x: f64,
        /// Baseline of the first line, mm (Y up).
        y: f64,
        /// Height of the capitals, mm (default 10).
        #[serde(default)]
        height_mm: Option<f64>,
        /// Layer id (default: the current layer; a black layer when there is none).
        #[serde(default)]
        layer: Option<u32>,
    }
}

with_text_style! {
    #[derive(Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct SetTextParams {
        /// Id of a text object.
        id: u64,
        /// New text.
        #[serde(default)]
        text: Option<String>,
        /// New height of the capitals, mm.
        #[serde(default)]
        height_mm: Option<f64>,
        /// On a path: where the text starts along it, mm.
        #[serde(default)]
        path_offset_mm: Option<f64>,
        /// On a path: distance of the baseline from it, mm.
        #[serde(default)]
        path_distance_mm: Option<f64>,
        /// On a path: follow it backwards (the other side).
        #[serde(default)]
        path_flip: Option<bool>,
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TextOnPathParams {
    /// Id of a text object.
    id: u64,
    /// Id of the shape whose first path the text follows (it follows it when it
    /// changes).
    path: u64,
    /// Where the text starts along the path, mm (default 0).
    #[serde(default)]
    offset_mm: Option<f64>,
    /// Distance of the baseline from the path, mm (default 0).
    #[serde(default)]
    distance_mm: Option<f64>,
    /// Follow the path backwards (default false).
    #[serde(default)]
    flip: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TextIdParams {
    /// Id of a text object.
    id: u64,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct FontPreviewParams {
    /// Font family.
    family: String,
    /// Sample text (default `AaBbÇç 123`).
    #[serde(default)]
    text: Option<String>,
    /// Bold.
    #[serde(default)]
    bold: bool,
    /// Italic.
    #[serde(default)]
    italic: bool,
}

/// A font family of `fonts.list`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FontFamily {
    /// Family name (a text's `font`).
    pub name: String,
    /// Single-line (open strokes, one pass per stroke).
    pub single_line: bool,
    /// Built into FreeKerf.
    pub builtin: bool,
    /// Has a bold face (else bold is drawn thicker).
    pub bold: bool,
    /// Has an italic face (else italic is slanted).
    pub italic: bool,
}

/// A sample of a font (`fonts.preview`).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FontPreview {
    /// SVG path data of the sample 10 mm high, mm, Y down (for `<path d>`).
    pub d: String,
    /// `[x, y, width, height]` of the sample, mm, Y down (for `viewBox`).
    pub view_box: [f64; 4],
    /// Single-line: draw with a stroke, not a fill.
    pub single_line: bool,
}

/// A transform of `document.transform`.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
enum TransformOp {
    /// Move by `[dx, dy]` mm.
    Translate {
        /// `[dx, dy]`, mm.
        translate: [f64; 2],
    },
    /// Scale by `[sx, sy]` about `pivot` (negative factors mirror).
    Scale {
        /// `[sx, sy]` (not zero).
        scale: [f64; 2],
        /// `[x, y]`, mm.
        pivot: [f64; 2],
    },
    /// Rotate counterclockwise by `rotate_deg` degrees about `pivot`.
    Rotate {
        /// Degrees, counterclockwise.
        rotate_deg: f64,
        /// `[x, y]`, mm.
        pivot: [f64; 2],
    },
    /// Any affine transform `[a, b, c, d, e, f]`: `x' = a·x + c·y + e`, `y' = b·x + d·y + f`.
    Matrix {
        /// `[a, b, c, d, e, f]`.
        matrix: [f64; 6],
    },
}

impl TransformOp {
    fn matrix(self) -> Result<Matrix, ActionError> {
        let finite = |v: &[f64]| v.iter().all(|x| x.is_finite());
        let bad = || ActionError::InvalidParams("the transform needs finite numbers".into());
        let m = match self {
            Self::Translate { translate: [dx, dy] } => {
                if !finite(&[dx, dy]) {
                    return Err(bad());
                }
                Matrix::translation(dx, dy)
            }
            Self::Scale {
                scale: [sx, sy],
                pivot: [px, py],
            } => {
                if !finite(&[sx, sy, px, py]) {
                    return Err(bad());
                }
                if sx == 0.0 || sy == 0.0 {
                    return Err(ActionError::InvalidParams("scale factors cannot be 0".into()));
                }
                let mut m = Matrix::translation(-px, -py);
                m.scale(sx, sy);
                m.translate(px, py);
                m
            }
            Self::Rotate {
                rotate_deg,
                pivot: [px, py],
            } => {
                if !finite(&[rotate_deg, px, py]) {
                    return Err(bad());
                }
                let mut m = Matrix::IDENTITY;
                m.rotate_at(rotate_deg, px, py);
                m
            }
            Self::Matrix {
                matrix: [a, b, c, d, e, f],
            } => {
                if !finite(&[a, b, c, d, e, f]) {
                    return Err(bad());
                }
                let m = Matrix::new(a, b, c, d, e, f);
                if m.determinant() == 0.0 {
                    return Err(ActionError::InvalidParams(
                        "the matrix flattens the drawing".into(),
                    ));
                }
                m
            }
        };
        Ok(m)
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TransformParams {
    /// The transform.
    op: TransformOp,
    /// Object ids (default: the selection).
    #[serde(default)]
    ids: Option<Vec<u64>>,
}

/// How `document.align` lines objects up (on the box around all of them).
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum AlignHow {
    /// Left edges.
    Left,
    /// Horizontal centres.
    CenterH,
    /// Right edges.
    Right,
    /// Top edges.
    Top,
    /// Vertical middles.
    MiddleV,
    /// Bottom edges.
    Bottom,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AlignParams {
    /// Which edge or centre.
    how: AlignHow,
    /// Object ids (default: the selection; at least two).
    #[serde(default)]
    ids: Option<Vec<u64>>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DuplicateParams {
    /// Object ids (default: the selection).
    #[serde(default)]
    ids: Option<Vec<u64>>,
    /// Offset of the copies, `[dx, dy]` mm (default `[5, -5]`: right and down).
    #[serde(default)]
    offset_mm: Option<[f64; 2]>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PasteParams {
    /// Offset of the copies, `[dx, dy]` mm (default: `[5, -5]` once more for every
    /// paste in the document copied from; none for the first paste in another
    /// document or after a cut).
    #[serde(default)]
    offset_mm: Option<[f64; 2]>,
    /// Centre of the pasted objects, `[x, y]` mm (e.g. where the context menu was
    /// opened; `offset_mm` wins when both are given).
    #[serde(default)]
    at_mm: Option<[f64; 2]>,
}

#[derive(Serialize, JsonSchema)]
struct Copied {
    /// Objects now on the clipboard (0: nothing was copied, the clipboard is unchanged).
    objects: usize,
}

#[derive(Serialize, JsonSchema)]
struct NewIds {
    /// Ids of what was created.
    ids: Vec<u64>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ColorParams {
    /// Colour, `#RRGGBB`.
    color: Color,
}

#[derive(Serialize, JsonSchema)]
struct LayerRef {
    /// Layer id.
    layer: u32,
    /// Whether the layer was created.
    created: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct MoveLayerParams {
    /// Layer id.
    id: u32,
    /// New position in the layer list (0 is first; past the end means last).
    to: usize,
}

/// Operations of `document.boolean`.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum BooleanOp {
    /// Weld: the area of any of them.
    Union,
    /// The first (in drawing order) minus the others.
    Difference,
    /// The area of all of them.
    Intersection,
    /// The area of exactly one of them.
    Xor,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct BooleanParams {
    /// The operation.
    op: BooleanOp,
    /// Object ids (default: the selection; at least two shapes).
    #[serde(default)]
    ids: Option<Vec<u64>>,
}

/// Corners of `document.offset`.
#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum OffsetJoin {
    /// Rounded (default).
    #[default]
    Round,
    /// Square.
    Square,
    /// Sharp (mitered up to twice the distance).
    Miter,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct OffsetParams {
    /// Distance, mm: positive grows, negative shrinks.
    distance_mm: f64,
    /// Corners.
    #[serde(default)]
    join: OffsetJoin,
    /// Object ids (default: the selection).
    #[serde(default)]
    ids: Option<Vec<u64>>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ArrayParams {
    /// Columns (at least 1).
    columns: u32,
    /// Rows (at least 1).
    rows: u32,
    /// Gap between copies, `[x, y]` mm (default `[5, 5]`); rows go down.
    #[serde(default)]
    gap_mm: Option<[f64; 2]>,
    /// Object ids (default: the selection).
    #[serde(default)]
    ids: Option<Vec<u64>>,
}

/// Most copies `document.array` makes.
const ARRAY_MAX_COPIES: u32 = 10_000;

/// Tolerance, mm, of curves turned into polygons for boolean operations and offsets.
const POLYGON_TOLERANCE_MM: f64 = 0.01;

/// The closed polygons of a shape object (open paths are closed).
fn polygons(o: &Object) -> Vec<Vec<Point>> {
    match &o.kind {
        ObjectKind::Shape { paths } => paths
            .iter()
            .map(|p| p.to_polyline(POLYGON_TOLERANCE_MM))
            .filter(|p| p.len() >= 3)
            .collect(),
        _ => Vec::new(),
    }
}

/// Polygons or polylines, mm.
type Polylines = Vec<Vec<Point>>;

/// An object's closed paths as polygons, and its open paths as polylines.
fn open_and_closed(o: &Object) -> (Polylines, Polylines) {
    let (mut closed, mut open) = (Vec::new(), Vec::new());
    if let ObjectKind::Shape { paths } = &o.kind {
        for p in paths {
            let pts = p.to_polyline(POLYGON_TOLERANCE_MM);
            let ends_meet = pts.len() > 2
                && pts
                    .first()
                    .zip(pts.last())
                    .is_some_and(|(a, b)| a.distance(*b) < 1e-9);
            if p.closed || ends_meet {
                if pts.len() >= 3 {
                    closed.push(pts);
                }
            } else if pts.len() >= 2 {
                open.push(pts);
            }
        }
    }
    (closed, open)
}

fn closed_paths(polys: Vec<Vec<Point>>) -> Vec<Path> {
    polys.into_iter().map(|p| Path::polyline(&p, true)).collect()
}

fn geometry_failed(e: impl std::fmt::Display) -> ActionError {
    ActionError::failed(format!("geometry: {e}"))
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ImageIdParams {
    /// Id of an image object.
    id: u64,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SetRasterParams {
    /// Id of an image object.
    id: u64,
    /// The raster dialog's settings (all of them, as `document.raster_settings` gives).
    settings: RasterSettings,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RasterPreviewParams {
    /// Id of an image object.
    id: u64,
    /// Settings to try (default: the image's own).
    #[serde(default)]
    settings: Option<RasterSettings>,
}

#[derive(Serialize, JsonSchema)]
struct RasterPreview {
    /// The bitmap as a PNG file, base64.
    png: String,
    /// Width in pixels.
    width: u32,
    /// Height in pixels.
    height: u32,
}

fn image_of(d: &Document, id: u64) -> Result<&ImageObject, ActionError> {
    match d.object(ObjectId(id)).map(|o| &o.kind) {
        Some(ObjectKind::Image(img)) => Ok(img),
        Some(_) => Err(ActionError::InvalidParams(format!("object {id} is not an image"))),
        None => Err(ActionError::InvalidParams(format!("no object {id}"))),
    }
}

fn check_raster(s: &RasterSettings) -> Result<(), ActionError> {
    if !(s.lines_per_mm > 0.0 && s.lines_per_mm.is_finite()) || s.min_power > s.max_power {
        return Err(ActionError::InvalidParams(
            "lines_per_mm must be positive and min_power at most max_power".into(),
        ));
    }
    Ok(())
}

/// How the cells of a material test are burned.
#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum TestMode {
    /// Filled squares (engraving).
    #[default]
    Fill,
    /// Square outlines (lines, cuts).
    Line,
}

/// A range of a material test: `steps` values from `from` to `to`.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TestRange {
    /// First value.
    from: f64,
    /// Last value.
    to: f64,
    /// Number of values (1 to 20).
    steps: u32,
}

impl TestRange {
    fn values(self, what: &str, min: f64, max: f64) -> Result<Vec<f64>, ActionError> {
        if !(1..=20).contains(&self.steps)
            || !(self.from.is_finite() && self.to.is_finite())
            || self.from.min(self.to) < min
            || self.from.max(self.to) > max
        {
            return Err(ActionError::InvalidParams(format!(
                "{what}: 1 to 20 steps between {min} and {max}"
            )));
        }
        let n = self.steps;
        Ok((0..n)
            .map(|i| {
                if n == 1 {
                    self.from
                } else {
                    self.from + (self.to - self.from) * f64::from(i) / f64::from(n - 1)
                }
            })
            .collect())
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct MaterialTestParams {
    /// Power, % of `max_s` (rows, bottom to top).
    power_pct: TestRange,
    /// Speed, mm/min (columns, left to right).
    speed_mm_min: TestRange,
    /// Filled squares or outlines (default: fill).
    #[serde(default)]
    mode: TestMode,
    /// Side of a cell, mm (default 10).
    #[serde(default)]
    cell_mm: Option<f64>,
    /// Gap between cells, mm (default 3).
    #[serde(default)]
    gap_mm: Option<f64>,
    /// Bottom-left corner of the grid, mm (default 0, 0).
    #[serde(default)]
    origin: Option<[f64; 2]>,
    /// The machine's `$30` (the `S` of 100 %; default 1000).
    #[serde(default)]
    max_s: Option<u32>,
}

#[derive(Serialize, JsonSchema)]
struct MaterialTest {
    /// Ids of the cells' layers, row by row from the bottom.
    layers: Vec<u32>,
    /// Ids of the new objects (cells, then labels).
    objects: Vec<u64>,
}

/// A colour for cell `i` of `n`: hues around the wheel, light enough to see.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn test_color(i: usize, n: usize) -> Color {
    let h = (i as f64 / n.max(1) as f64) * 6.0;
    let x = 1.0 - (h % 2.0 - 1.0).abs();
    let (r, g, b) = match h as u32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    };
    let c = |v: f64| (40.0 + v * 200.0).round() as u8;
    Color::rgb(c(r), c(g), c(b))
}

/// Label strokes (LaserGRBL's Hershey font) with their baseline-left at `(x, y)`.
fn label_at(text: &str, x: f64, y: f64, height_mm: f64) -> Vec<Path> {
    freekerf_text::hershey::text_polylines(text, x, y, height_mm)
        .into_iter()
        .map(|pts| {
            let pts: Vec<Point> = pts.into_iter().map(|(x, y)| Point::new(x, y)).collect();
            Path::polyline(&pts, false)
        })
        .collect()
}

/// Default height of new text, mm.
const TEXT_HEIGHT_MM: f64 = 10.0;

fn text_failed(e: &freekerf_text::TextError) -> ActionError {
    match e {
        freekerf_text::TextError::InvalidSpec(_) | freekerf_text::TextError::UnknownFont(_) => {
            ActionError::InvalidParams(e.to_string())
        }
        _ => ActionError::failed(e.to_string()),
    }
}

/// A text object of the document.
fn text_object(d: &mut Document, id: u64) -> Result<(&mut Object, TextSpec), ActionError> {
    let o = d
        .object_mut(ObjectId(id))
        .ok_or_else(|| ActionError::InvalidParams(format!("no object {id}")))?;
    let spec = o
        .text
        .clone()
        .ok_or_else(|| ActionError::InvalidParams(format!("object {id} is not text")))?;
    Ok((o, spec))
}

/// The layer of a new object: the one asked for, else the current one, else a new
/// black layer.
fn layer_for_new(
    d: &mut Document,
    asked: Option<u32>,
    current: Option<LayerId>,
) -> Result<LayerId, ActionError> {
    match asked {
        Some(id) if d.layer(LayerId(id)).is_some() => Ok(LayerId(id)),
        Some(id) => Err(ActionError::InvalidParams(format!("no layer {id}"))),
        None => Ok(current
            .filter(|id| d.layer(*id).is_some())
            .unwrap_or_else(|| d.layer_for_color(Color::BLACK, 1000, 1000))),
    }
}

/// Adds an object made in the editor (source `native`) and selects it alone.
fn add_native(
    d: &mut Document,
    sel: &mut BTreeSet<ObjectId>,
    layer: LayerId,
    kind: ObjectKind,
    name: &str,
) -> Result<ObjectId, ActionError> {
    let id = d
        .add_object_with(
            layer,
            kind,
            name.to_owned(),
            Some(Source {
                format: SourceFormat::Native,
                file: String::new(),
            }),
        )
        .map_err(failed)?;
    *sel = BTreeSet::from([id]);
    Ok(id)
}

fn bounds_of_paths(paths: &[Path]) -> Rect {
    let mut r = Rect::EMPTY;
    for p in paths {
        r.union(&p.bounds());
    }
    r
}

fn bounds_of<'a>(objects: impl Iterator<Item = &'a Object>) -> Rect {
    let mut r = Rect::EMPTY;
    for o in objects {
        r.union(&o.bounds());
    }
    r
}

/// Imports a file's bytes into `doc`: drawings by colour layer (an existing layer of
/// the same colour is reused) or all on `current` when unified, images on a layer
/// of their own at the origin.
///
/// # Errors
/// The file cannot be read as `format`.
#[allow(clippy::cast_precision_loss)]
fn import_into(
    doc: &mut Document,
    p: &ImportParams,
    bytes: &[u8],
    current: Option<LayerId>,
    fonts: &freekerf_text::Fonts,
) -> Result<Vec<ObjectId>, ActionError> {
    let (speed, power) = (p.speed_mm_min.unwrap_or(1000), p.power.unwrap_or(1000));
    let (format, imported) = match p.format {
        ImportFormat::Svg => (
            SourceFormat::Svg,
            freekerf_import::svg::import(
                bytes,
                &freekerf_import::svg::SvgOptions {
                    fonts: Some(fonts.clone()),
                    ..freekerf_import::svg::SvgOptions::default()
                },
            )
            .map_err(failed)?,
        ),
        ImportFormat::Dxf => (
            SourceFormat::Dxf,
            freekerf_import::dxf::import(bytes, &freekerf_import::dxf::DxfOptions::default())
                .map_err(failed)?,
        ),
        ImportFormat::Image => {
            let img = freekerf_raster::image::Image::decode(bytes).map_err(failed)?;
            let dpi = p.dpi.filter(|d| *d > 0.0).unwrap_or(96.0);
            let mm = |px: u32| f64::from(px) * 25.4 / dpi;
            let layer = doc.add_layer(Layer::new(LayerId(0), Color::BLACK, speed, power));
            let id = doc
                .add_object_with(
                    layer,
                    ObjectKind::Image(Box::new(ImageObject {
                        data: ImageData::Encoded {
                            media_type: "image/*".into(),
                            bytes: Bytes(bytes.to_vec()),
                        },
                        position: Point::new(0.0, 0.0),
                        size_mm: [mm(img.width()), mm(img.height())],
                        settings: RasterSettings::default(),
                        edits: Vec::new(),
                    })),
                    p.name.clone(),
                    Some(Source {
                        format: SourceFormat::Image,
                        file: p.name.clone(),
                    }),
                )
                .map_err(failed)?;
            return Ok(vec![id]);
        }
    };
    let source = Source {
        format,
        file: p.name.clone(),
    };
    let unified = match p.colors {
        ImportColors::Keep => None,
        ImportColors::Unify => Some(layer_for_new(doc, None, current)?),
    };
    let mut ids = Vec::new();
    for o in &imported.objects {
        let layer = unified.unwrap_or_else(|| doc.layer_for_color(o.color, speed, power));
        ids.push(
            doc.add_object_with(layer, o.kind.clone(), String::new(), Some(source.clone()))
                .map_err(failed)?,
        );
    }
    let added: Vec<Option<ObjectId>> = ids.iter().copied().map(Some).collect();
    freekerf_import::attach_texts(doc, &imported.objects, &added);
    Ok(ids)
}

/// Imports a file into the session as one undo step, the format taken from the
/// name's extension (`.svg`, `.dxf`, or an image). Returns the new object ids.
///
/// # Errors
/// Unknown extension, or a file that cannot be read.
pub fn import_file(session: &mut Session, name: &str, bytes: &[u8]) -> Result<Vec<u64>, ActionError> {
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    let format = match ext.as_str() {
        "svg" => ImportFormat::Svg,
        "dxf" => ImportFormat::Dxf,
        "png" | "jpg" | "jpeg" | "bmp" | "gif" => ImportFormat::Image,
        _ => {
            return Err(ActionError::InvalidParams(format!(
                "cannot import `.{ext}` files"
            )));
        }
    };
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name).to_owned();
    let p = ImportParams {
        format,
        data: None,
        text: None,
        name: base,
        dpi: None,
        speed_mm_min: None,
        power: None,
        colors: ImportColors::Keep,
    };
    let mut ids = Vec::new();
    let fonts = session.fonts().clone();
    session.edit("import", |d, _| {
        ids = import_into(d, &p, bytes, None, &fonts)?;
        Ok::<_, ActionError>(!ids.is_empty())
    })?;
    Ok(ids.into_iter().map(|i| i.0).collect())
}

/// Registers the session actions into `r`.
///
/// # Errors
/// A name clash with an action the host registered first.
#[allow(clippy::too_many_lines)] // one registration per action
pub fn register_session_actions<C: SessionHost>(r: &mut Registry<C>) -> Result<(), RegistryError> {
    // ---- state
    r.register(
        "session.snapshot",
        "The session as front ends and agents read it: revision, selection, view, undo/redo labels, layers and objects without geometry.",
        Tier::Read,
        |c: &C, _: NoParams| -> Result<Snapshot, ActionError> { Ok(lock(c).snapshot()) },
    )?;
    r.register(
        "document.get",
        "The whole document (layers, objects with geometry and images), as in a `.fkproj` file.",
        Tier::Read,
        |c: &C, _: NoParams| Ok(lock(c).document().clone()),
    )?;
    r.register(
        "document.to_fkproj",
        "The document as `.fkproj` text.",
        Tier::Read,
        |c: &C, _: NoParams| {
            Ok(FkprojText {
                text: lock(c).document().to_fkproj(),
            })
        },
    )?;

    // ---- document
    r.register(
        "document.new",
        "Starts an empty document (the undo history starts again).",
        Tier::Edit,
        |c: &C, _: NoParams| {
            lock(c).replace_document(Document::new());
            Ok(Changed { changed: true })
        },
    )?;
    r.register(
        "document.from_fkproj",
        "Replaces the document with a `.fkproj` text (the undo history starts again).",
        Tier::Edit,
        |c: &C, p: FkprojParams| {
            let doc = Document::from_fkproj(p.text.as_bytes()).map_err(failed)?;
            lock(c).replace_document(doc);
            Ok(Changed { changed: true })
        },
    )?;
    r.register(
        "document.import_data",
        "Imports a file given as base64 bytes (`data`), or as `text` for SVG and DXF, into the document: SVG and DXF by colour layer (or all on the current layer), images on a layer of their own. Returns the new objects and layers. One undo step.",
        Tier::Edit,
        |c: &C, p: ImportParams| {
            let bytes = match (&p.data, &p.text) {
                (Some(data), None) => base64::engine::general_purpose::STANDARD
                    .decode(data.as_bytes())
                    .map_err(|e| ActionError::InvalidParams(format!("data: {e}")))?,
                (None, Some(text)) if !matches!(p.format, ImportFormat::Image) => text.clone().into_bytes(),
                (None, Some(_)) => {
                    return Err(ActionError::InvalidParams("images need `data` (base64)".into()));
                }
                _ => return Err(ActionError::InvalidParams("give exactly one of `data` and `text`".into())),
            };
            let mut s = lock(c);
            let current = s.current_layer();
            let before: Vec<LayerId> = s.document().layers.iter().map(|l| l.id).collect();
            let fonts = s.fonts().clone();
            let mut ids = Vec::new();
            s.edit("import", |d, _| {
                ids = import_into(d, &p, &bytes, current, &fonts)?;
                Ok::<_, ActionError>(!ids.is_empty())
            })?;
            Ok(Imported::new(s.document(), &before, ids))
        },
    )?;
    r.register(
        "document.delete",
        "Deletes objects (default: the selection). One undo step.",
        Tier::Edit,
        |c: &C, p: IdsParams| {
            let mut s = lock(c);
            let ids = targets(p.ids, &s);
            let changed = s.edit("delete", |d, _| {
                let mut any = false;
                for id in &ids {
                    any |= d.remove_object(*id).is_ok();
                }
                Ok::<_, ActionError>(any)
            })?;
            Ok(Changed { changed })
        },
    )?;
    r.register(
        "document.translate",
        "Moves objects (default: the selection) by `dx`, `dy` mm. One undo step.",
        Tier::Edit,
        |c: &C, p: TranslateParams| {
            if !(p.dx.is_finite() && p.dy.is_finite()) {
                return Err(ActionError::InvalidParams("dx and dy must be finite".into()));
            }
            let mut s = lock(c);
            let ids = targets(p.ids, &s);
            let changed = s.edit("move", |d, _| {
                let mut any = false;
                for id in &ids {
                    if let Some(o) = d.object_mut(*id) {
                        o.translate(p.dx, p.dy);
                        any = true;
                    }
                }
                Ok::<_, ActionError>(any && (p.dx != 0.0 || p.dy != 0.0))
            })?;
            Ok(Changed { changed })
        },
    )?;
    r.register(
        "document.move_to_layer",
        "Moves objects (default: the selection) to another layer. One undo step.",
        Tier::Edit,
        |c: &C, p: MoveToLayerParams| {
            let mut s = lock(c);
            let ids = targets(p.ids, &s);
            let changed = s.edit("move to layer", |d, _| {
                if d.layer(LayerId(p.layer)).is_none() {
                    return Err(ActionError::InvalidParams(format!("no layer {}", p.layer)));
                }
                let mut any = false;
                for id in &ids {
                    if d.object(*id).is_some_and(|o| o.layer != LayerId(p.layer)) {
                        d.set_object_layer(*id, LayerId(p.layer)).map_err(failed)?;
                        any = true;
                    }
                }
                Ok(any)
            })?;
            Ok(Changed { changed })
        },
    )?;
    r.register(
        "document.add_layer",
        "Adds a layer of a colour (line, 1000 mm/min, power 1000, 1 pass unless given). One undo step.",
        Tier::Edit,
        |c: &C, p: AddLayerParams| {
            let mut id = 0;
            lock(c).edit("add layer", |d, _| {
                let mut l = Layer::new(LayerId(0), p.color, 1000, 1000);
                p.fields.apply(&mut l)?;
                id = d.add_layer(l).0;
                Ok::<_, ActionError>(true)
            })?;
            Ok(NewId { id: u64::from(id) })
        },
    )?;
    r.register(
        "document.update_layer",
        "Changes a layer's name, mode, speed, power, passes, visibility, filling or laser mode (M3/M4). One undo step.",
        Tier::Edit,
        |c: &C, p: UpdateLayerParams| {
            let changed = lock(c).edit("layer settings", |d, _| {
                let l = d
                    .layer_mut(LayerId(p.id))
                    .ok_or_else(|| ActionError::InvalidParams(format!("no layer {}", p.id)))?;
                let before = l.clone();
                p.fields.apply(l)?;
                Ok(*l != before)
            })?;
            Ok(Changed { changed })
        },
    )?;
    r.register(
        "document.remove_layer",
        "Removes a layer and its objects. One undo step.",
        Tier::Edit,
        |c: &C, p: LayerIdParams| {
            lock(c).edit("remove layer", |d, _| {
                d.remove_layer(LayerId(p.id)).map_err(failed)?;
                Ok::<_, ActionError>(true)
            })?;
            Ok(Changed { changed: true })
        },
    )?;

    r.register(
        "document.add_shape",
        "Draws a rectangle or an ellipse on a layer (default: the current layer) and selects it. One undo step.",
        Tier::Edit,
        |c: &C, p: AddShapeParams| {
            if ![p.x, p.y, p.width, p.height].iter().all(|v| v.is_finite()) || p.width <= 0.0 || p.height <= 0.0 {
                return Err(ActionError::InvalidParams(
                    "x and y must be finite, width and height positive".into(),
                ));
            }
            let mut s = lock(c);
            let current = s.current_layer();
            let mut id = ObjectId(0);
            s.edit("add shape", |d, sel| {
                let layer = layer_for_new(d, p.layer, current)?;
                let (path, name) = match p.shape {
                    ShapeKind::Rect => (rect_path(p.x, p.y, p.width, p.height), "rectangle"),
                    ShapeKind::Ellipse => (ellipse_path(p.x, p.y, p.width, p.height), "ellipse"),
                };
                id = add_native(d, sel, layer, ObjectKind::Shape { paths: vec![path] }, name)?;
                Ok::<_, ActionError>(true)
            })?;
            Ok(NewId { id: id.0 })
        },
    )?;
    r.register(
        "document.add_text",
        "Writes text on a layer (default: the current layer) and selects it: a font of `fonts.list` (default Relief SingleLine, single-line), its style, alignment, spacing and effects. The text stays editable (`document.set_text`). One undo step.",
        Tier::Edit,
        |c: &C, p: AddTextParams| {
            if !(p.x.is_finite() && p.y.is_finite()) {
                return Err(ActionError::InvalidParams("x and y must be finite".into()));
            }
            let mut spec = TextSpec::new(
                p.text.clone(),
                p.height_mm.unwrap_or(TEXT_HEIGHT_MM),
                freekerf_text::DEFAULT_FONT,
            );
            p.style(&mut spec);
            freekerf_text::check(&spec).map_err(|e| text_failed(&e))?;
            let mut s = lock(c);
            let current = s.current_layer();
            let fonts = s.fonts().clone();
            let mut id = ObjectId(0);
            s.edit("add text", |d, sel| {
                let layer = layer_for_new(d, p.layer, current)?;
                id = add_native(d, sel, layer, ObjectKind::Shape { paths: Vec::new() }, "text")?;
                if let Some(o) = d.object_mut(id) {
                    o.transform = Matrix::translation(p.x, p.y);
                    freekerf_text::apply(o, spec, &fonts).map_err(|e| text_failed(&e))?;
                }
                Ok::<_, ActionError>(true)
            })?;
            Ok(NewId { id: id.0 })
        },
    )?;
    r.register(
        "document.set_text",
        "Changes a text object: its text, height, font, style, alignment, spacing, effects, or where it sits on its path (its position, rotation and scale stay). One undo step.",
        Tier::Edit,
        |c: &C, p: SetTextParams| {
            let mut s = lock(c);
            let fonts = s.fonts().clone();
            let changed = s.edit("edit text", |d, _| {
                let (o, old) = text_object(d, p.id)?;
                let mut spec = old.clone();
                if let Some(t) = &p.text {
                    spec.text.clone_from(t);
                }
                spec.height_mm = p.height_mm.unwrap_or(spec.height_mm);
                p.style(&mut spec);
                if let Some(tp) = spec.path.as_mut() {
                    tp.offset_mm = p.path_offset_mm.unwrap_or(tp.offset_mm);
                    tp.distance_mm = p.path_distance_mm.unwrap_or(tp.distance_mm);
                    tp.flip = p.path_flip.unwrap_or(tp.flip);
                    // a text on a path is not bent as well
                    spec.arc_radius_mm = None;
                } else if p.path_offset_mm.is_some() || p.path_distance_mm.is_some() || p.path_flip.is_some() {
                    return Err(ActionError::InvalidParams(format!("text {} is not on a path", p.id)));
                }
                if spec == old {
                    return Ok(false);
                }
                freekerf_text::apply(o, spec, &fonts).map_err(|e| text_failed(&e))?;
                Ok::<_, ActionError>(true)
            })?;
            Ok(Changed { changed })
        },
    )?;
    r.register(
        "document.text_on_path",
        "Puts a text on the first path of a shape: the letters stand along it, and follow it whenever the shape changes (moved, scaled, edited); moving the text alone puts it back on the path. Deleting the shape leaves the text where it was. One undo step.",
        Tier::Edit,
        |c: &C, p: TextOnPathParams| {
            let mut s = lock(c);
            let fonts = s.fonts().clone();
            s.edit("text on path", |d, _| {
                if p.id == p.path {
                    return Err(ActionError::InvalidParams("a text cannot follow itself".into()));
                }
                let baseline = d
                    .object(ObjectId(p.path))
                    .ok_or_else(|| ActionError::InvalidParams(format!("no object {}", p.path)))
                    .and_then(|o| {
                        freekerf_text::baseline_of(o)
                            .cloned()
                            .ok_or_else(|| ActionError::InvalidParams(format!("object {} is not a shape", p.path)))
                    })?;
                let (o, mut spec) = text_object(d, p.id)?;
                spec.arc_radius_mm = None;
                spec.path = Some(TextPath {
                    object: Some(ObjectId(p.path)),
                    path: baseline,
                    offset_mm: p.offset_mm.unwrap_or(0.0),
                    distance_mm: p.distance_mm.unwrap_or(0.0),
                    flip: p.flip.unwrap_or(false),
                });
                freekerf_text::apply(o, spec, &fonts).map_err(|e| text_failed(&e))?;
                Ok::<_, ActionError>(true)
            })?;
            Ok(Changed { changed: true })
        },
    )?;
    r.register(
        "document.text_off_path",
        "Takes a text off its path: straight again, its baseline starting where the path starts. One undo step.",
        Tier::Edit,
        |c: &C, p: TextIdParams| {
            let mut s = lock(c);
            let fonts = s.fonts().clone();
            let changed = s.edit("text off path", |d, _| {
                let (o, mut spec) = text_object(d, p.id)?;
                let Some(tp) = spec.path.take() else {
                    return Ok(false);
                };
                let start = o.transform.transform(if tp.flip { tp.path.end() } else { tp.path.start });
                o.transform = Matrix::translation(start.x, start.y);
                freekerf_text::apply(o, spec, &fonts).map_err(|e| text_failed(&e))?;
                Ok::<_, ActionError>(true)
            })?;
            Ok(Changed { changed })
        },
    )?;
    r.register(
        "document.text_to_paths",
        "Turns texts (default: the selected ones) into plain shapes: the letters stay, the text is no longer editable. One undo step.",
        Tier::Edit,
        |c: &C, p: IdsParams| {
            let mut s = lock(c);
            let ids = targets(p.ids, &s);
            let changed = s.edit("text to paths", |d, _| {
                let mut any = false;
                for o in d.objects.iter_mut().filter(|o| ids.contains(&o.id) && o.text.is_some()) {
                    o.text = None;
                    o.name = "path".into();
                    any = true;
                }
                Ok::<_, ActionError>(any)
            })?;
            Ok(Changed { changed })
        },
    )?;
    r.register(
        "fonts.list",
        "The font families a text can use: the built-in single-line fonts first, then the installed ones.",
        Tier::Read,
        |c: &C, _: NoParams| {
            Ok(lock(c)
                .fonts()
                .families()
                .into_iter()
                .map(|f| FontFamily {
                    name: f.name,
                    single_line: f.single_line,
                    builtin: f.builtin,
                    bold: f.bold,
                    italic: f.italic,
                })
                .collect::<Vec<_>>())
        },
    )?;
    r.register(
        "fonts.preview",
        "A sample of a font family as SVG path data (10 mm high, Y down), for font pickers.",
        Tier::Read,
        |c: &C, p: FontPreviewParams| {
            let fonts = lock(c).fonts().clone();
            let mut spec = TextSpec::new(
                p.text.unwrap_or_else(|| "AaBbÇç 123".into()),
                TEXT_HEIGHT_MM,
                p.family.clone(),
            );
            spec.bold = p.bold;
            spec.italic = p.italic;
            let paths = freekerf_text::layout(&spec, &fonts).map_err(|e| text_failed(&e))?;
            let single_line = fonts
                .face(&p.family, p.bold, p.italic)
                .map_err(|e| text_failed(&e))?
                .face
                .single_line();
            let b = bounds_of_paths(&paths);
            let view_box = if b.is_empty() {
                [0.0, -TEXT_HEIGHT_MM, TEXT_HEIGHT_MM, TEXT_HEIGHT_MM]
            } else {
                [b.min_x, -b.max_y, b.width(), b.height()]
            };
            Ok(FontPreview {
                d: freekerf_text::svgpath::to_data(&paths, true),
                view_box,
                single_line,
            })
        },
    )?;
    r.register(
        "document.transform",
        "Moves, scales, mirrors or rotates objects (default: the selection): `{translate}`, `{scale, pivot}`, `{rotate_deg, pivot}` or `{matrix}`. Images only move and scale. One undo step.",
        Tier::Edit,
        |c: &C, p: TransformParams| {
            let m = p.op.matrix()?;
            let mut s = lock(c);
            let ids = targets(p.ids, &s);
            let changed = s.edit("transform", |d, _| {
                let mut any = false;
                for id in &ids {
                    if let Some(o) = d.object_mut(*id) {
                        o.apply_transform(&m).map_err(|e| ActionError::InvalidParams(e.to_string()))?;
                        any = true;
                    }
                }
                Ok::<_, ActionError>(any && m != Matrix::IDENTITY)
            })?;
            Ok(Changed { changed })
        },
    )?;
    r.register(
        "document.align",
        "Lines up objects (default: the selection, at least two) on the box around them: left, center_h, right, top, middle_v or bottom. One undo step.",
        Tier::Edit,
        |c: &C, p: AlignParams| {
            let mut s = lock(c);
            let ids = targets(p.ids, &s);
            let changed = s.edit("align", |d, _| {
                let all = bounds_of(d.objects.iter().filter(|o| ids.contains(&o.id)));
                if ids.len() < 2 || all.is_empty() {
                    return Ok(false);
                }
                let mut any = false;
                for id in &ids {
                    let Some(o) = d.object_mut(*id) else { continue };
                    let b = o.bounds();
                    if b.is_empty() {
                        continue;
                    }
                    let (dx, dy) = match p.how {
                        AlignHow::Left => (all.min_x - b.min_x, 0.0),
                        AlignHow::Right => (all.max_x - b.max_x, 0.0),
                        AlignHow::CenterH => ((all.min_x + all.max_x - b.min_x - b.max_x) / 2.0, 0.0),
                        AlignHow::Top => (0.0, all.max_y - b.max_y),
                        AlignHow::Bottom => (0.0, all.min_y - b.min_y),
                        AlignHow::MiddleV => (0.0, (all.min_y + all.max_y - b.min_y - b.max_y) / 2.0),
                    };
                    if dx != 0.0 || dy != 0.0 {
                        o.translate(dx, dy);
                        any = true;
                    }
                }
                Ok::<_, ActionError>(any)
            })?;
            Ok(Changed { changed })
        },
    )?;
    r.register(
        "document.duplicate",
        "Copies objects (default: the selection), offset by `offset_mm`, and selects the copies. One undo step.",
        Tier::Edit,
        |c: &C, p: DuplicateParams| {
            let [dx, dy] = p.offset_mm.unwrap_or([5.0, -5.0]);
            if !(dx.is_finite() && dy.is_finite()) {
                return Err(ActionError::InvalidParams("offset_mm must be finite".into()));
            }
            let mut s = lock(c);
            let ids = targets(p.ids, &s);
            let mut new = Vec::new();
            s.edit("duplicate", |d, sel| {
                let copies: Vec<Object> = d.objects.iter().filter(|o| ids.contains(&o.id)).cloned().collect();
                let mut pairs = Vec::new();
                for mut o in copies {
                    o.translate(dx, dy);
                    let id = d
                        .add_object_with(o.layer, o.kind.clone(), o.name.clone(), o.source.clone())
                        .map_err(failed)?;
                    if let Some(added) = d.object_mut(id) {
                        added.text = o.text;
                        added.transform = o.transform;
                    }
                    pairs.push((o.id, id));
                    new.push(id);
                }
                freekerf_text::relink_copies(d, &pairs);
                if new.is_empty() {
                    return Ok(false);
                }
                *sel = new.iter().copied().collect();
                Ok::<_, ActionError>(true)
            })?;
            Ok(NewIds {
                ids: new.into_iter().map(|i| i.0).collect(),
            })
        },
    )?;
    r.register(
        "document.copy",
        "Puts copies of objects (default: the selection) and their layers on the session's clipboard, which survives opening another document. Not an undo step.",
        Tier::Ui,
        |c: &C, p: IdsParams| {
            let mut s = lock(c);
            let ids = targets(p.ids, &s);
            Ok(Copied {
                objects: s.copy(&ids, false),
            })
        },
    )?;
    r.register(
        "document.cut",
        "Puts objects (default: the selection) on the clipboard like `document.copy` and deletes them; the next paste puts them back in place. One undo step.",
        Tier::Edit,
        |c: &C, p: IdsParams| {
            let mut s = lock(c);
            let ids = targets(p.ids, &s);
            let objects = s.copy(&ids, true);
            if objects > 0 {
                s.edit("cut", |d, _| {
                    for id in &ids {
                        // ids that do not exist were not copied either
                        d.remove_object(*id).ok();
                    }
                    Ok::<_, ActionError>(true)
                })?;
            }
            Ok(Copied { objects })
        },
    )?;
    r.register(
        "document.paste",
        "Adds the clipboard's objects (`document.copy`, `document.cut`) and selects them: each on the layer of its colour, created with the copied settings when missing. Offset by `offset_mm`, or centred on `at_mm`, by default `[5, -5]` mm once more for every paste in the document they were copied from. One undo step.",
        Tier::Edit,
        |c: &C, p: PasteParams| {
            if [p.offset_mm, p.at_mm]
                .iter()
                .flatten()
                .any(|v| !v.iter().all(|x| x.is_finite()))
            {
                return Err(ActionError::InvalidParams("offset_mm and at_mm must be finite".into()));
            }
            let ids = lock(c).paste([5.0, -5.0], p.offset_mm, p.at_mm).map_err(failed)?;
            Ok(NewIds {
                ids: ids.into_iter().map(|i| i.0).collect(),
            })
        },
    )?;
    r.register(
        "document.combine",
        "Joins shapes (default: the selection) into one object per layer, so they move together; open paths whose ends touch (0.01 mm) become one continuous path, closed when it comes back to its start. Texts and images are left as they are. The results are selected. One undo step.",
        Tier::Edit,
        |c: &C, p: IdsParams| {
            let mut s = lock(c);
            let ids = targets(p.ids, &s);
            let mut new = Vec::new();
            s.edit("combine", |d, sel| {
                let chosen: Vec<Object> = d
                    .objects
                    .iter()
                    .filter(|o| ids.contains(&o.id) && o.text.is_none() && matches!(o.kind, ObjectKind::Shape { .. }))
                    .cloned()
                    .collect();
                let mut layers: Vec<LayerId> = Vec::new();
                for o in &chosen {
                    if !layers.contains(&o.layer) {
                        layers.push(o.layer);
                    }
                }
                for layer in layers {
                    let group: Vec<&Object> = chosen.iter().filter(|o| o.layer == layer).collect();
                    let paths: Vec<Path> = group
                        .iter()
                        .flat_map(|o| match &o.kind {
                            ObjectKind::Shape { paths } => paths.clone(),
                            _ => Vec::new(),
                        })
                        .collect();
                    let count = paths.len();
                    let joined = freekerf_geometry::join_paths(paths, freekerf_geometry::JOIN_TOLERANCE_MM);
                    // one object whose paths do not touch: nothing to do
                    if group.len() < 2 && joined.len() == count {
                        continue;
                    }
                    for o in &group {
                        d.remove_object(o.id).map_err(failed)?;
                    }
                    // the first one's name and source (the source sets the writing style)
                    let first = group[0];
                    let id = d
                        .add_object_with(
                            layer,
                            ObjectKind::Shape { paths: joined },
                            first.name.clone(),
                            first.source.clone(),
                        )
                        .map_err(failed)?;
                    new.push(id);
                }
                if new.is_empty() {
                    return Ok(false);
                }
                *sel = new.iter().copied().collect();
                Ok::<_, ActionError>(true)
            })?;
            Ok(NewIds {
                ids: new.into_iter().map(|i| i.0).collect(),
            })
        },
    )?;
    r.register(
        "document.split",
        "Splits shapes (default: the selection) made of several paths into one object per path, on the same layer; texts and images are left as they are. The pieces are selected. One undo step.",
        Tier::Edit,
        |c: &C, p: IdsParams| {
            let mut s = lock(c);
            let ids = targets(p.ids, &s);
            let mut new = Vec::new();
            s.edit("split", |d, sel| {
                let chosen: Vec<Object> = d
                    .objects
                    .iter()
                    .filter(|o| {
                        ids.contains(&o.id)
                            && o.text.is_none()
                            && matches!(&o.kind, ObjectKind::Shape { paths } if paths.len() > 1)
                    })
                    .cloned()
                    .collect();
                for o in chosen {
                    d.remove_object(o.id).map_err(failed)?;
                    let ObjectKind::Shape { paths } = o.kind else {
                        continue;
                    };
                    for path in paths {
                        let id = d
                            .add_object_with(o.layer, ObjectKind::Shape { paths: vec![path] }, o.name.clone(), o.source.clone())
                            .map_err(failed)?;
                        if let Some(piece) = d.object_mut(id) {
                            piece.transform = o.transform;
                        }
                        new.push(id);
                    }
                }
                if new.is_empty() {
                    return Ok(false);
                }
                *sel = new.iter().copied().collect();
                Ok::<_, ActionError>(true)
            })?;
            Ok(NewIds {
                ids: new.into_iter().map(|i| i.0).collect(),
            })
        },
    )?;
    r.register(
        "document.boolean",
        "Combines shapes (default: the selection, at least two) into one: union (weld), difference (the first in drawing order minus the others), intersection or xor. The result takes the first one's layer and is selected. One undo step.",
        Tier::Edit,
        |c: &C, p: BooleanParams| {
            let mut s = lock(c);
            let ids = targets(p.ids, &s);
            let mut new = ObjectId(0);
            s.edit("boolean", |d, sel| {
                let chosen: Vec<&Object> = d
                    .objects
                    .iter()
                    .filter(|o| ids.contains(&o.id) && matches!(o.kind, ObjectKind::Shape { .. }))
                    .collect();
                if chosen.len() < 2 {
                    return Err(ActionError::InvalidParams("choose at least two shapes".into()));
                }
                let (first, rest) = (chosen[0], &chosen[1..]);
                let (layer, name) = (first.layer, first.name.clone());
                let scale = freekerf_geometry::boolean::DEFAULT_SCALE;
                let all = |objs: &[&Object]| objs.iter().flat_map(|o| polygons(o)).collect::<Vec<_>>();
                let op = |a: &[Vec<Point>], b: &[Vec<Point>], op| {
                    freekerf_geometry::boolean::boolean(a, b, op, scale).map_err(geometry_failed)
                };
                use freekerf_geometry::boolean::Op;
                let result = match p.op {
                    BooleanOp::Union => op(&all(&chosen), &[], Op::Union)?,
                    BooleanOp::Difference => op(&polygons(first), &all(rest), Op::Difference)?,
                    BooleanOp::Xor => op(&polygons(first), &all(rest), Op::Xor)?,
                    BooleanOp::Intersection => {
                        let mut acc = polygons(first);
                        for o in rest {
                            acc = op(&acc, &polygons(o), Op::Intersection)?;
                        }
                        acc
                    }
                };
                let gone: Vec<ObjectId> = chosen.iter().map(|o| o.id).collect();
                for id in gone {
                    d.remove_object(id).map_err(failed)?;
                }
                new = add_native(d, sel, layer, ObjectKind::Shape { paths: closed_paths(result) }, &name)?;
                Ok::<_, ActionError>(true)
            })?;
            Ok(NewId { id: new.0 })
        },
    )?;
    r.register(
        "document.offset",
        "Adds the outline of shapes (default: the selection) grown or shrunk by `distance_mm`, as new objects on the same layers, selected; open paths (single-line text) are outlined around their strokes when growing. One undo step.",
        Tier::Edit,
        |c: &C, p: OffsetParams| {
            if !(p.distance_mm.is_finite() && p.distance_mm != 0.0) {
                return Err(ActionError::InvalidParams("distance_mm must be finite and not 0".into()));
            }
            let mut s = lock(c);
            let ids = targets(p.ids, &s);
            let join = match p.join {
                OffsetJoin::Round => freekerf_geometry::boolean::Join::Round {
                    arc_tolerance: POLYGON_TOLERANCE_MM,
                },
                OffsetJoin::Square => freekerf_geometry::boolean::Join::Square,
                OffsetJoin::Miter => freekerf_geometry::boolean::Join::Miter { limit: 2.0 },
            };
            let mut new = Vec::new();
            s.edit("offset", |d, sel| {
                let scale = freekerf_geometry::boolean::DEFAULT_SCALE;
                // closed paths grow or shrink; open ones (single-line text) are
                // outlined, with round ends, when growing
                let sources: Vec<(LayerId, Polylines, Polylines)> = d
                    .objects
                    .iter()
                    .filter(|o| ids.contains(&o.id))
                    .map(|o| {
                        let (closed, open) = open_and_closed(o);
                        (o.layer, closed, open)
                    })
                    .filter(|(_, c, o)| !c.is_empty() || !o.is_empty())
                    .collect();
                for (layer, closed, open) in sources {
                    let mut out = freekerf_geometry::boolean::offset(&closed, p.distance_mm, join, scale)
                        .map_err(geometry_failed)?;
                    if p.distance_mm > 0.0 && !open.is_empty() {
                        let strokes =
                            freekerf_geometry::boolean::offset_open(&open, p.distance_mm, POLYGON_TOLERANCE_MM, scale)
                                .map_err(geometry_failed)?;
                        out.extend(strokes);
                        out = freekerf_geometry::boolean::boolean(&out, &[], freekerf_geometry::boolean::Op::Union, scale)
                            .map_err(geometry_failed)?;
                    }
                    if out.is_empty() {
                        continue;
                    }
                    new.push(add_native(d, sel, layer, ObjectKind::Shape { paths: closed_paths(out) }, "offset")?);
                }
                if new.is_empty() {
                    return Ok(false);
                }
                *sel = new.iter().copied().collect();
                Ok::<_, ActionError>(true)
            })?;
            Ok(NewIds {
                ids: new.into_iter().map(|i| i.0).collect(),
            })
        },
    )?;
    r.register(
        "document.array",
        "Copies objects (default: the selection) in a grid of `columns` × `rows` (the originals are the top-left cell; rows go down), `gap_mm` apart; selects the originals and the copies. One undo step.",
        Tier::Edit,
        |c: &C, p: ArrayParams| {
            let [gx, gy] = p.gap_mm.unwrap_or([5.0, 5.0]);
            if p.columns == 0 || p.rows == 0 || !(gx.is_finite() && gy.is_finite()) {
                return Err(ActionError::InvalidParams(
                    "columns and rows must be at least 1, the gap finite".into(),
                ));
            }
            if p.columns.saturating_mul(p.rows) > ARRAY_MAX_COPIES {
                return Err(ActionError::InvalidParams(format!(
                    "at most {ARRAY_MAX_COPIES} cells"
                )));
            }
            let mut s = lock(c);
            let ids = targets(p.ids, &s);
            let mut new = Vec::new();
            s.edit("array", |d, sel| {
                let originals: Vec<Object> = d.objects.iter().filter(|o| ids.contains(&o.id)).cloned().collect();
                let b = bounds_of(originals.iter());
                if originals.is_empty() || b.is_empty() {
                    return Ok(false);
                }
                let (sx, sy) = (b.width() + gx, b.height() + gy);
                for row in 0..p.rows {
                    for col in 0..p.columns {
                        if row == 0 && col == 0 {
                            continue;
                        }
                        let mut pairs = Vec::new();
                        for o in &originals {
                            let mut copy = o.clone();
                            copy.translate(f64::from(col) * sx, -f64::from(row) * sy);
                            let id = d
                                .add_object_with(copy.layer, copy.kind, copy.name, copy.source)
                                .map_err(failed)?;
                            if let Some(added) = d.object_mut(id) {
                                added.text = copy.text;
                                added.transform = copy.transform;
                            }
                            pairs.push((o.id, id));
                            new.push(id);
                        }
                        // each cell's texts follow that cell's paths
                        freekerf_text::relink_copies(d, &pairs);
                    }
                }
                *sel = originals.iter().map(|o| o.id).chain(new.iter().copied()).collect();
                Ok::<_, ActionError>(!new.is_empty())
            })?;
            Ok(NewIds {
                ids: new.into_iter().map(|i| i.0).collect(),
            })
        },
    )?;
    r.register(
        "document.raster_settings",
        "The raster dialog's settings of an image object (tool, dithering, resolution, grey adjustments, speeds, powers).",
        Tier::Read,
        |c: &C, p: ImageIdParams| Ok(image_of(lock(c).document(), p.id)?.settings.clone()),
    )?;
    r.register(
        "document.set_raster",
        "Changes the raster dialog's settings of an image object. One undo step.",
        Tier::Edit,
        |c: &C, p: SetRasterParams| {
            check_raster(&p.settings)?;
            let changed = lock(c).edit("image settings", |d, _| {
                image_of(d, p.id)?;
                match d.object_mut(ObjectId(p.id)).map(|o| &mut o.kind) {
                    Some(ObjectKind::Image(img)) if img.settings != p.settings => {
                        img.settings = p.settings.clone();
                        Ok(true)
                    }
                    _ => Ok::<_, ActionError>(false),
                }
            })?;
            Ok(Changed { changed })
        },
    )?;
    r.register(
        "document.raster_preview",
        "The bitmap an image object's raster settings (or the ones given) make: grey levels, or black and white when dithered; a PNG, base64.",
        Tier::Read,
        |c: &C, p: RasterPreviewParams| {
            let img = image_of(lock(c).document(), p.id)?.clone();
            let settings = p.settings.unwrap_or_else(|| img.settings.clone());
            check_raster(&settings)?;
            let bitmap = freekerf_planner::image::preview_bitmap(&img, &settings).map_err(failed)?;
            let png = bitmap.encode_png().map_err(failed)?;
            Ok(RasterPreview {
                png: base64::engine::general_purpose::STANDARD.encode(png),
                width: bitmap.width(),
                height: bitmap.height(),
            })
        },
    )?;
    r.register(
        "document.material_test",
        "Draws a material test (design/ux.md flow 6): a grid of cells, power rising by row and speed by column, each cell on a layer of its own with that power and speed, labelled in single-stroke text on a layer of low power. One undo step.",
        Tier::Edit,
        |c: &C, p: MaterialTestParams| {
            let powers = p.power_pct.values("power_pct", 0.0, 100.0)?;
            let speeds = p.speed_mm_min.values("speed_mm_min", 1.0, 100_000.0)?;
            let (cell, gap) = (p.cell_mm.unwrap_or(10.0), p.gap_mm.unwrap_or(3.0));
            if !(cell >= 1.0 && cell.is_finite() && gap >= 0.0 && gap.is_finite()) {
                return Err(ActionError::InvalidParams("cell_mm at least 1, gap_mm at least 0".into()));
            }
            let [ox, oy] = p.origin.unwrap_or([0.0, 0.0]);
            let max_s = f64::from(p.max_s.unwrap_or(1000));
            let label_h = (cell * 0.3).clamp(2.0, 5.0);
            // room on the left for the power labels
            let left = ox + label_h * 4.0;
            let mut s = lock(c);
            let mut out = MaterialTest {
                layers: Vec::new(),
                objects: Vec::new(),
            };
            s.edit("material test", |d, sel| {
                let n = powers.len() * speeds.len();
                let labels = d.add_layer({
                    let mut l = Layer::new(LayerId(0), Color::BLACK, 3000, 0);
                    l.name = "test labels".into();
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    {
                        l.power = (max_s * 0.2).round() as u32;
                    }
                    l
                });
                let mut ids = Vec::new();
                for (row, pct) in powers.iter().enumerate() {
                    #[allow(clippy::cast_precision_loss)]
                    let y = oy + row as f64 * (cell + gap);
                    for (col, speed) in speeds.iter().enumerate() {
                        #[allow(clippy::cast_precision_loss)]
                        let x = left + col as f64 * (cell + gap);
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        let mut l = Layer::new(
                            LayerId(0),
                            test_color(row * speeds.len() + col, n),
                            speed.round() as u32,
                            (pct / 100.0 * max_s).round() as u32,
                        );
                        l.name = format!("{pct:.0}% · {speed:.0} mm/min");
                        l.mode = match p.mode {
                            TestMode::Fill => LayerMode::Fill,
                            TestMode::Line => LayerMode::Line,
                        };
                        let layer = d.add_layer(l);
                        out.layers.push(layer.0);
                        let name = format!("{pct:.0}% {speed:.0}");
                        ids.push(add_native(
                            d,
                            sel,
                            layer,
                            ObjectKind::Shape {
                                paths: vec![rect_path(x, y, cell, cell)],
                            },
                            &name,
                        )?);
                    }
                    let label = label_at(&format!("{pct:.0}%"), ox, y + (cell - label_h) / 2.0, label_h);
                    ids.push(add_native(d, sel, labels, ObjectKind::Shape { paths: label }, "label")?);
                }
                #[allow(clippy::cast_precision_loss)]
                let top = oy + powers.len() as f64 * (cell + gap) + gap;
                for (col, speed) in speeds.iter().enumerate() {
                    #[allow(clippy::cast_precision_loss)]
                    let x = left + col as f64 * (cell + gap);
                    // speeds in tenths of a metre per minute fit a cell: "1.5" for 1500
                    let text = format!("{}", (speed / 100.0).round() / 10.0);
                    ids.push(add_native(d, sel, labels, ObjectKind::Shape { paths: label_at(&text, x, top, label_h) }, "label")?);
                }
                let title = label_at("F m/min - S %", left, top + label_h * 1.8, label_h);
                ids.push(add_native(d, sel, labels, ObjectKind::Shape { paths: title }, "label")?);
                *sel = ids.iter().copied().collect();
                out.objects = ids.iter().map(|i| i.0).collect();
                Ok::<_, ActionError>(true)
            })?;
            Ok(out)
        },
    )?;
    r.register(
        "document.layer_for_color",
        "The layer of a colour, created (line, 1000 mm/min, power 1000) when missing; it becomes the current layer. Creating it is one undo step.",
        Tier::Edit,
        |c: &C, p: ColorParams| {
            let mut s = lock(c);
            let mut layer = s.document().layer_by_color(p.color).map(|l| l.id);
            let created = layer.is_none();
            if created {
                s.edit("add layer", |d, _| {
                    layer = Some(d.layer_for_color(p.color, 1000, 1000));
                    Ok::<_, ActionError>(true)
                })?;
            }
            let layer = layer.ok_or_else(|| failed("no layer"))?;
            s.set_current_layer(layer);
            Ok(LayerRef {
                layer: layer.0,
                created,
            })
        },
    )?;
    r.register(
        "document.set_current_layer",
        "Chooses the layer new shapes and text go to.",
        Tier::Ui,
        |c: &C, p: LayerIdParams| {
            if lock(c).set_current_layer(LayerId(p.id)) {
                Ok(Changed { changed: true })
            } else {
                Err(ActionError::InvalidParams(format!("no layer {}", p.id)))
            }
        },
    )?;
    r.register(
        "document.move_layer",
        "Moves a layer in the layer list (the order of layers of the same mode is the order they run). One undo step.",
        Tier::Edit,
        |c: &C, p: MoveLayerParams| {
            let changed = lock(c).edit("move layer", |d, _| {
                let from = d
                    .layers
                    .iter()
                    .position(|l| l.id == LayerId(p.id))
                    .ok_or_else(|| ActionError::InvalidParams(format!("no layer {}", p.id)))?;
                let to = p.to.min(d.layers.len() - 1);
                if from == to {
                    return Ok(false);
                }
                let l = d.layers.remove(from);
                d.layers.insert(to, l);
                Ok::<_, ActionError>(true)
            })?;
            Ok(Changed { changed })
        },
    )?;

    // ---- selection
    r.register(
        "document.select",
        "Selects objects (replace, add to or remove from the selection). Unknown ids are ignored.",
        Tier::Ui,
        |c: &C, p: SelectParams| {
            let mut s = lock(c);
            let ids: BTreeSet<ObjectId> = p.ids.into_iter().map(ObjectId).collect();
            let mut sel = s.selection().clone();
            match p.mode {
                SelectMode::Replace => sel = ids,
                SelectMode::Add => sel.extend(ids),
                SelectMode::Remove => sel.retain(|i| !ids.contains(i)),
                SelectMode::Toggle => {
                    for id in ids {
                        if !sel.remove(&id) {
                            sel.insert(id);
                        }
                    }
                }
            }
            s.select(sel);
            Ok(s.selection().iter().map(|i| i.0).collect::<Vec<_>>())
        },
    )?;
    r.register(
        "document.select_all",
        "Selects every object.",
        Tier::Ui,
        |c: &C, _: NoParams| {
            let mut s = lock(c);
            let all = s.document().objects.iter().map(|o| o.id).collect();
            s.select(all);
            Ok(s.selection().len())
        },
    )?;
    r.register(
        "document.clear_selection",
        "Selects nothing.",
        Tier::Ui,
        |c: &C, _: NoParams| {
            lock(c).select(BTreeSet::new());
            Ok(Changed { changed: true })
        },
    )?;

    // ---- history
    r.register(
        "history.undo",
        "Undoes the last document change.",
        Tier::Edit,
        |c: &C, _: NoParams| Ok(HistoryResult { step: lock(c).undo() }),
    )?;
    r.register(
        "history.redo",
        "Redoes the last undone change.",
        Tier::Edit,
        |c: &C, _: NoParams| Ok(HistoryResult { step: lock(c).redo() }),
    )?;

    // ---- view (the desktop app renders it; scripts and agents drive the same screen)
    r.register(
        "view.get",
        "The view: mode, tool, panel, dialogs, toasts, viewport, focus, the pending laser request.",
        Tier::Read,
        |c: &C, _: NoParams| -> Result<ViewState, ActionError> { Ok(lock(c).view().clone()) },
    )?;
    r.register(
        "view.set_mode",
        "Switches between design and produce (produce shows the machine panel).",
        Tier::Ui,
        |c: &C, p: ModeParams| view_unit(c, ViewCommand::SetMode(p.mode)),
    )?;
    r.register(
        "view.set_tool",
        "Chooses the Design mode tool (select, rectangle, ellipse, text).",
        Tier::Ui,
        |c: &C, p: ToolParams| view_unit(c, ViewCommand::SetTool(p.tool)),
    )?;
    r.register(
        "view.open_panel",
        "Shows a panel of the dock (`layers`, `object`, `machine`, `console`; opens the drawer in the medium layout).",
        Tier::Ui,
        |c: &C, p: PanelParams| view_unit(c, ViewCommand::OpenPanel(p.name)),
    )?;
    r.register(
        "view.close_panel",
        "Closes the drawer when it shows this panel.",
        Tier::Ui,
        |c: &C, p: PanelParams| view_unit(c, ViewCommand::ClosePanel(p.name)),
    )?;
    r.register(
        "view.open_dialog",
        "Opens a dialog on top of the others (dialogs are not modal: STOP stays reachable).",
        Tier::Ui,
        |c: &C, p: DialogParams| view_unit(c, ViewCommand::OpenDialog(p.name)),
    )?;
    r.register(
        "view.close_dialog",
        "Closes a dialog (default: the top one). Closing the fire confirmation drops its laser request.",
        Tier::Ui,
        |c: &C, p: OptionalDialogParams| view_unit(c, ViewCommand::CloseDialog(p.name)),
    )?;
    r.register(
        "view.toast",
        "Shows a short message (at most 3 at once); returns its id.",
        Tier::Ui,
        |c: &C, p: ToastParams| {
            view_cmd(
                c,
                ViewCommand::Toast {
                    level: p.level,
                    message: p.message,
                    args: p.args,
                },
            )
            .map(|id| id.unwrap_or(0))
        },
    )?;
    r.register(
        "view.dismiss_toast",
        "Removes a toast.",
        Tier::Ui,
        |c: &C, p: ToastIdParams| view_unit(c, ViewCommand::DismissToast(p.id)),
    )?;
    r.register(
        "view.set_viewport",
        "Centres the view on a point (mm) at a zoom (CSS pixels per mm).",
        Tier::Ui,
        |c: &C, p: Viewport| view_unit(c, ViewCommand::SetViewport(p)),
    )?;
    r.register(
        "view.fit",
        "Fits the work area in the canvas.",
        Tier::Ui,
        |c: &C, _: NoParams| view_unit(c, ViewCommand::Fit),
    )?;
    r.register(
        "view.focus",
        "Records which element has the keyboard focus.",
        Tier::Ui,
        |c: &C, p: FocusParams| view_unit(c, ViewCommand::Focus(p.target)),
    )?;
    r.describe_result::<ViewState>("view.get")?;
    r.describe_result::<u64>("view.toast")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::Caller;

    struct Host(Mutex<Session>);

    impl SessionHost for Host {
        fn session(&self) -> &Mutex<Session> {
            &self.0
        }
    }

    fn setup() -> (Registry<Host>, Host) {
        let mut r = Registry::new();
        register_session_actions(&mut r).unwrap();
        (r, Host(Mutex::new(Session::new())))
    }

    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="20mm" height="20mm" viewBox="0 0 20 20">
        <rect x="1" y="1" width="5" height="5" stroke="red" fill="none"/><path d="M1 1 L5 5" stroke="#00f"/></svg>"##;

    fn b64(b: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(b)
    }

    #[test]
    fn raster_settings_and_preview() {
        let (r, h) = setup();
        let user = Caller::user();
        let d = |name: &str, p: Value| r.dispatch(&h, &user, name, p);
        // a 20 × 10 grey ramp
        let mut px = Vec::new();
        for _y in 0..10 {
            for x in 0..20u8 {
                px.extend_from_slice(&[x * 12, x * 12, x * 12, 255]);
            }
        }
        let png = freekerf_raster::image::Image::from_rgba(20, 10, &px)
            .unwrap()
            .encode_png()
            .unwrap();
        let id = d(
            "document.import_data",
            json!({"format": "image", "data": b64(&png), "dpi": 25.4}),
        )
        .unwrap()["objects"][0]
            .clone();
        let snap = d("session.snapshot", Value::Null).unwrap();
        assert_eq!(snap["document"]["objects"][0]["image"]["tool"], "line_to_line");
        let mut settings = d("document.raster_settings", json!({"id": id})).unwrap();
        assert_eq!(settings["lines_per_mm"], 3.0);
        // grey preview, then dithered: only black and white
        let grey = d("document.raster_preview", json!({"id": id})).unwrap();
        assert_eq!(
            (grey["width"].as_u64(), grey["height"].as_u64()),
            (Some(60), Some(30))
        );
        settings["tool"] = json!("dithering");
        settings["dithering"] = json!("atkinson");
        let dithered = d("document.raster_preview", json!({"id": id, "settings": settings})).unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(dithered["png"].as_str().unwrap())
            .unwrap();
        let img = freekerf_raster::image::Image::decode(&bytes).unwrap();
        assert!(
            img.pixels()
                .iter()
                .all(|&p| matches!(freekerf_raster::image::split(p).1, 0 | 255))
        );
        // nothing changed until set_raster
        assert_eq!(
            d("session.snapshot", Value::Null).unwrap()["document"]["objects"][0]["image"]["tool"],
            "line_to_line"
        );
        assert_eq!(
            d("document.set_raster", json!({"id": id, "settings": settings})).unwrap()["changed"],
            true
        );
        let snap = d("session.snapshot", Value::Null).unwrap();
        assert_eq!(snap["document"]["objects"][0]["image"]["dithering"], "atkinson");
        assert_eq!(snap["undo"].as_array().unwrap().last().unwrap(), "image settings");
        assert_eq!(
            d("document.set_raster", json!({"id": id, "settings": settings})).unwrap()["changed"],
            false
        );
        // errors
        settings["lines_per_mm"] = json!(0.0);
        assert!(d("document.set_raster", json!({"id": id, "settings": settings})).is_err());
        assert!(d("document.raster_preview", json!({"id": id, "settings": settings})).is_err());
        let rect = d(
            "document.add_shape",
            json!({"shape": "rect", "x": 0, "y": 0, "width": 1, "height": 1}),
        )
        .unwrap()["id"]
            .clone();
        assert!(d("document.raster_settings", json!({"id": rect})).is_err());
        assert!(d("document.raster_settings", json!({"id": 999})).is_err());
    }

    #[test]
    fn material_test_grid() {
        let (r, h) = setup();
        let user = Caller::user();
        let d = |name: &str, p: Value| r.dispatch(&h, &user, name, p);
        let out = d(
            "document.material_test",
            json!({"power_pct": {"from": 20, "to": 100, "steps": 3}, "speed_mm_min": {"from": 500, "to": 3000, "steps": 2}, "max_s": 255}),
        )
        .unwrap();
        assert_eq!(out["layers"].as_array().unwrap().len(), 6);
        let snap = d("session.snapshot", Value::Null).unwrap();
        let layers = snap["document"]["layers"].as_array().unwrap().clone();
        assert_eq!(layers.len(), 7, "six cells and the labels");
        let cell = |power: u64, speed: u64| {
            layers
                .iter()
                .any(|l| l["power"] == power && l["speed_mm_min"] == speed && l["mode"] == "fill")
        };
        assert!(cell(51, 500) && cell(153, 3000) && cell(255, 500), "{layers:?}");
        // every cell has its own colour
        let mut colors: Vec<&str> = layers.iter().map(|l| l["color"].as_str().unwrap()).collect();
        colors.sort_unstable();
        colors.dedup();
        assert_eq!(colors.len(), 7);
        // 6 cells, 3 power labels, 2 speed labels, a title; all selected; one step
        assert_eq!(out["objects"].as_array().unwrap().len(), 12);
        assert_eq!(snap["selection"].as_array().unwrap().len(), 12);
        assert_eq!(snap["undo"], json!(["material test"]));
        // outlines, one step each way
        let lines = d(
            "document.material_test",
            json!({"power_pct": {"from": 50, "to": 50, "steps": 1}, "speed_mm_min": {"from": 1000, "to": 1000, "steps": 1}, "mode": "line", "origin": [100, 0]}),
        )
        .unwrap();
        let layer = lines["layers"][0].as_u64().unwrap();
        let snap = d("session.snapshot", Value::Null).unwrap();
        let l = snap["document"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["id"] == layer)
            .cloned()
            .unwrap();
        assert_eq!(
            (l["mode"].as_str(), l["power"].as_u64()),
            (Some("line"), Some(500))
        );
        for bad in [
            json!({"power_pct": {"from": 0, "to": 120, "steps": 3}, "speed_mm_min": {"from": 100, "to": 200, "steps": 2}}),
            json!({"power_pct": {"from": 10, "to": 20, "steps": 0}, "speed_mm_min": {"from": 100, "to": 200, "steps": 2}}),
            json!({"power_pct": {"from": 10, "to": 20, "steps": 21}, "speed_mm_min": {"from": 100, "to": 200, "steps": 2}}),
            json!({"power_pct": {"from": 10, "to": 20, "steps": 2}, "speed_mm_min": {"from": 0, "to": 200, "steps": 2}}),
            json!({"power_pct": {"from": 10, "to": 20, "steps": 2}, "speed_mm_min": {"from": 100, "to": 200, "steps": 2}, "cell_mm": 0.5}),
        ] {
            assert!(d("document.material_test", bad.clone()).is_err(), "{bad}");
        }
    }

    #[test]
    fn boolean_offset_and_array() {
        let (r, h) = setup();
        let user = Caller::user();
        let d = |name: &str, p: Value| r.dispatch(&h, &user, name, p);
        let objects = || d("session.snapshot", Value::Null).unwrap()["document"]["objects"].clone();
        let bounds_of = |id: &Value| -> [f64; 4] {
            let o = objects()
                .as_array()
                .unwrap()
                .iter()
                .find(|o| o["id"] == *id)
                .cloned()
                .unwrap();
            serde_json::from_value(o["bounds"].clone()).unwrap()
        };
        let close = |a: [f64; 4], b: [f64; 4]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3);
        let a = d(
            "document.add_shape",
            json!({"shape": "rect", "x": 0, "y": 0, "width": 10, "height": 10}),
        )
        .unwrap()["id"]
            .clone();
        let b = d(
            "document.add_shape",
            json!({"shape": "rect", "x": 5, "y": 0, "width": 10, "height": 10}),
        )
        .unwrap()["id"]
            .clone();

        // weld: one object over both
        let u = d("document.boolean", json!({"op": "union", "ids": [a, b]})).unwrap()["id"].clone();
        assert_eq!(objects().as_array().unwrap().len(), 1);
        assert!(close(bounds_of(&u), [0.0, 0.0, 15.0, 10.0]));
        assert_eq!(
            d("session.snapshot", Value::Null).unwrap()["selection"],
            json!([u])
        );
        d("history.undo", Value::Null).unwrap();
        // the first minus the others
        let diff = d("document.boolean", json!({"op": "difference", "ids": [a, b]})).unwrap()["id"].clone();
        assert!(close(bounds_of(&diff), [0.0, 0.0, 5.0, 10.0]));
        d("history.undo", Value::Null).unwrap();
        let inter =
            d("document.boolean", json!({"op": "intersection", "ids": [a, b]})).unwrap()["id"].clone();
        assert!(close(bounds_of(&inter), [5.0, 0.0, 10.0, 10.0]));
        d("history.undo", Value::Null).unwrap();
        let xor = d("document.boolean", json!({"op": "xor", "ids": [a, b]})).unwrap()["id"].clone();
        assert!(close(bounds_of(&xor), [0.0, 0.0, 15.0, 10.0]));
        d("history.undo", Value::Null).unwrap();
        assert!(d("document.boolean", json!({"op": "union", "ids": [a]})).is_err());
        let text = d("document.add_text", json!({"text": "I", "x": 0, "y": 0})).unwrap()["id"].clone();
        assert!(d("document.boolean", json!({"op": "union", "ids": [text, 999]})).is_err());

        // offset: a new outline around, the original stays
        let grown = d("document.offset", json!({"distance_mm": 2, "ids": [a]})).unwrap()["ids"][0].clone();
        assert!(close(bounds_of(&grown), [-2.0, -2.0, 12.0, 12.0]));
        let shrunk = d(
            "document.offset",
            json!({"distance_mm": -1, "join": "miter", "ids": [a]}),
        )
        .unwrap()["ids"][0]
            .clone();
        assert!(close(bounds_of(&shrunk), [1.0, 1.0, 9.0, 9.0]));
        d(
            "document.offset",
            json!({"distance_mm": 1, "join": "square", "ids": [a]}),
        )
        .unwrap();
        assert!(d("document.offset", json!({"distance_mm": 0, "ids": [a]})).is_err());
        // open strokes (single-line text) are outlined when growing, nothing when shrinking
        let around =
            d("document.offset", json!({"distance_mm": 1, "ids": [text]})).unwrap()["ids"][0].clone();
        let (tb, ab) = (bounds_of(&text), bounds_of(&around));
        assert!(ab[0] < tb[0] - 0.9 && ab[3] > tb[3] + 0.9, "{tb:?} {ab:?}");
        assert_eq!(
            d("document.offset", json!({"distance_mm": -1, "ids": [text]})).unwrap()["ids"],
            json!([])
        );

        // array: 3 × 2 of a 10 mm square with 5 mm gaps
        let before = objects().as_array().unwrap().len();
        let copies =
            d("document.array", json!({"columns": 3, "rows": 2, "ids": [b]})).unwrap()["ids"].clone();
        let copies = copies.as_array().unwrap().clone();
        assert_eq!(copies.len(), 5);
        assert_eq!(objects().as_array().unwrap().len(), before + 5);
        assert!(close(bounds_of(&copies[1]), [35.0, 0.0, 45.0, 10.0]));
        assert!(close(bounds_of(&copies[4]), [35.0, -15.0, 45.0, -5.0]));
        assert_eq!(
            d("session.snapshot", Value::Null).unwrap()["selection"]
                .as_array()
                .unwrap()
                .len(),
            6
        );
        assert!(d("document.array", json!({"columns": 0, "rows": 2})).is_err());
        assert!(d("document.array", json!({"columns": 1000, "rows": 1000})).is_err());
    }

    #[test]
    fn text_styles_paths_and_fonts() {
        let (r, h) = setup();
        let user = Caller::user();
        let d = |name: &str, p: Value| r.dispatch(&h, &user, name, p);
        let object = |id: &Value| {
            d("session.snapshot", Value::Null).unwrap()["document"]["objects"]
                .as_array()
                .unwrap()
                .iter()
                .find(|o| o["id"] == *id)
                .cloned()
                .unwrap()
        };
        let bounds =
            |id: &Value| -> [f64; 4] { serde_json::from_value(object(id)["bounds"].clone()).unwrap() };

        // fonts: built-in first; a sample to show
        let fonts = d("fonts.list", Value::Null).unwrap();
        assert_eq!(fonts[0]["name"], "Relief SingleLine");
        assert!(
            fonts
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["name"] == "Hershey Serif" && f["italic"] == true)
        );
        let sample = d("fonts.preview", json!({"family": "Hershey Script"})).unwrap();
        assert!(sample["d"].as_str().unwrap().starts_with('M') && sample["single_line"] == true);
        assert!(d("fonts.preview", json!({"family": "Nope"})).is_err());

        // a styled text in one call; the style is kept and changed field by field
        let text = d(
            "document.add_text",
            json!({"text": "Olá\nmundo", "x": 0, "y": 0, "font": "Hershey Serif", "bold": true,
                   "align": "center", "underline": true}),
        )
        .unwrap()["id"]
            .clone();
        let spec = object(&text)["text"].clone();
        assert_eq!(spec["font"], "Hershey Serif");
        assert_eq!(
            (spec["bold"].clone(), spec["align"].clone()),
            (json!(true), json!("center"))
        );
        let b = bounds(&text);
        assert!(
            b[0] < -5.0 && b[2] > 5.0 && b[1] < -10.0,
            "centred, two lines: {b:?}"
        );
        d(
            "document.set_text",
            json!({"id": text, "italic": true, "letter_spacing_mm": 1, "outline_mm": 0.5}),
        )
        .unwrap();
        let spec = object(&text)["text"].clone();
        assert_eq!(
            (spec["italic"].clone(), spec["bold"].clone()),
            (json!(true), json!(true))
        );
        d(
            "document.set_text",
            json!({"id": text, "outline_mm": 0, "arc_radius_mm": 40}),
        )
        .unwrap();
        let spec = object(&text)["text"].clone();
        assert!(spec.get("outline_mm").is_none() && spec["arc_radius_mm"] == 40.0);
        for bad in [
            json!({"id": text, "font": "Nope"}),
            json!({"id": text, "line_spacing": 0}),
            json!({"id": text, "path_offset_mm": 3}),
        ] {
            assert!(matches!(
                d("document.set_text", bad),
                Err(ActionError::InvalidParams(_))
            ));
        }
        assert!(
            d(
                "document.add_text",
                json!({"text": "x", "x": 0, "y": 0, "width_scale": -1})
            )
            .is_err()
        );

        // on a path: the text follows the shape, and goes back to it when moved alone
        let line = d(
            "document.add_shape",
            json!({"shape": "rect", "x": 0, "y": 50, "width": 200, "height": 1}),
        )
        .unwrap()["id"]
            .clone();
        let on = d("document.add_text", json!({"text": "ABC", "x": 0, "y": 0})).unwrap()["id"].clone();
        d(
            "document.text_on_path",
            json!({"id": on, "path": line, "offset_mm": 10}),
        )
        .unwrap();
        let b = bounds(&on);
        assert!(b[1] > 49.0 && b[0] > 9.0 && b[0] < 15.0, "{b:?}");
        assert_eq!(object(&on)["text"]["path"]["object"], line);
        d(
            "document.transform",
            json!({"ids": [line], "op": {"translate": [0, 30]}}),
        )
        .unwrap();
        assert!(bounds(&on)[1] > 79.0, "followed the shape");
        d(
            "document.transform",
            json!({"ids": [on], "op": {"translate": [100, 100]}}),
        )
        .unwrap();
        let back = bounds(&on);
        assert!(back[1] > 79.0 && back[1] < 81.0 && back[0] < 15.0, "{back:?}");
        // both moved together: the text moves once
        d(
            "document.transform",
            json!({"ids": [line, on], "op": {"translate": [0, 10]}}),
        )
        .unwrap();
        assert!((bounds(&on)[1] - back[1] - 10.0).abs() < 0.5);
        // undo takes the path and the text back in one step
        d("history.undo", Value::Null).unwrap();
        assert!((bounds(&on)[1] - back[1]).abs() < 1e-6);
        d(
            "document.set_text",
            json!({"id": on, "path_distance_mm": 2, "path_flip": true}),
        )
        .unwrap();
        assert_eq!(object(&on)["text"]["path"]["flip"], true);
        assert!(d("document.text_on_path", json!({"id": on, "path": on})).is_err());
        assert!(d("document.text_on_path", json!({"id": on, "path": text})).is_err());
        assert!(d("document.text_on_path", json!({"id": line, "path": on})).is_err());

        // copies: with the shape they follow the copy, alone they follow nothing
        let both = d("document.duplicate", json!({"ids": [line, on]})).unwrap()["ids"].clone();
        assert_eq!(object(&both[1])["text"]["path"]["object"], both[0]);
        let alone = d("document.duplicate", json!({"ids": [on]})).unwrap()["ids"][0].clone();
        assert!(object(&alone)["text"]["path"].get("object").is_none());

        // the shape deleted: the text stays where it was
        let kept = bounds(&on);
        d("document.delete", json!({"ids": [line]})).unwrap();
        assert!(object(&on)["text"]["path"].get("object").is_none());
        assert_eq!(bounds(&on), kept);
        // off the path: straight again, from where the path started
        assert_eq!(
            d("document.text_off_path", json!({"id": on})).unwrap()["changed"],
            true
        );
        assert!(object(&on)["text"].get("path").is_none());
        assert_eq!(
            d("document.text_off_path", json!({"id": on})).unwrap()["changed"],
            false
        );

        // texts become plain shapes
        assert_eq!(
            d("document.text_to_paths", json!({"ids": [on, line]})).unwrap()["changed"],
            true
        );
        assert_eq!(object(&on)["kind"], "shape");
        assert_eq!(
            d("document.text_to_paths", json!({"ids": [on]})).unwrap()["changed"],
            false
        );
    }

    #[test]
    fn editing_actions() {
        let (r, h) = setup();
        let user = Caller::user();
        let d = |name: &str, p: Value| r.dispatch(&h, &user, name, p);
        let bounds = |id: &Value| {
            let snap = d("session.snapshot", Value::Null).unwrap();
            let o = snap["document"]["objects"]
                .as_array()
                .unwrap()
                .iter()
                .find(|o| o["id"] == *id)
                .cloned()
                .unwrap();
            let b: [f64; 4] = serde_json::from_value(o["bounds"].clone()).unwrap();
            (b, o)
        };
        let close = |a: [f64; 4], b: [f64; 4]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6);

        // shapes: a black layer is made when there is none, and it becomes current
        let rect = d(
            "document.add_shape",
            json!({"shape": "rect", "x": 0, "y": 0, "width": 10, "height": 5}),
        )
        .unwrap()["id"]
            .clone();
        let snap = d("session.snapshot", Value::Null).unwrap();
        assert_eq!(snap["selection"], json!([rect]));
        assert_eq!(snap["current_layer"], snap["document"]["layers"][0]["id"]);
        assert_eq!(bounds(&rect).1["kind"], "shape");
        assert!(close(bounds(&rect).0, [0.0, 0.0, 10.0, 5.0]));
        let ell = d(
            "document.add_shape",
            json!({"shape": "ellipse", "x": 20, "y": 0, "width": 10, "height": 10}),
        )
        .unwrap()["id"]
            .clone();
        assert!(close(bounds(&ell).0, [20.0, 0.0, 30.0, 10.0]));
        assert!(
            d(
                "document.add_shape",
                json!({"shape": "rect", "x": 0, "y": 0, "width": 0, "height": 5})
            )
            .is_err()
        );
        assert!(
            d(
                "document.add_shape",
                json!({"shape": "rect", "x": 0, "y": 0, "width": 1, "height": 5, "layer": 99})
            )
            .is_err()
        );

        // colour layers and the current layer
        let red = d("document.layer_for_color", json!({"color": "#FF0000"})).unwrap();
        assert_eq!(red["created"], true);
        assert_eq!(
            d("document.layer_for_color", json!({"color": "#FF0000"})).unwrap()["created"],
            false
        );
        let snap = d("session.snapshot", Value::Null).unwrap();
        assert_eq!(snap["current_layer"], red["layer"]);
        let text = d(
            "document.add_text",
            json!({"text": "Hi", "x": 5, "y": 5, "height_mm": 10}),
        )
        .unwrap()["id"]
            .clone();
        let (tb, to) = bounds(&text);
        assert_eq!(to["kind"], "text");
        assert_eq!(to["layer"], red["layer"]);
        assert_eq!(
            to["text"],
            json!({"text": "Hi", "height_mm": 10.0, "font": "Relief SingleLine"})
        );
        assert!(
            // capitals 10 mm high, the dot of the i a little higher
            tb[0] >= 5.0 - 1e-9 && tb[1] >= 5.0 - 1e-9 && tb[3] <= 17.0,
            "{tb:?}"
        );
        let first = snap["document"]["layers"][0]["id"].clone();
        d("document.set_current_layer", json!({"id": first})).unwrap();
        assert_eq!(
            d("session.snapshot", Value::Null).unwrap()["current_layer"],
            first
        );
        assert!(d("document.set_current_layer", json!({"id": 99})).is_err());

        // text stays editable through transforms
        d(
            "document.transform",
            json!({"ids": [text], "op": {"rotate_deg": 90, "pivot": [5, 5]}}),
        )
        .unwrap();
        let (rb, ro) = bounds(&text);
        assert!(rb[2] <= 5.0 + 1e-9, "rotated to the left of the pivot: {rb:?}");
        let t: [f64; 6] = serde_json::from_value(ro["transform"].clone()).unwrap();
        assert!((t[1] - 1.0).abs() < 1e-9, "{t:?}");
        d("document.set_text", json!({"id": text, "text": "Hello"})).unwrap();
        let (hb, ho) = bounds(&text);
        assert_eq!(ho["text"]["text"], "Hello");
        assert!(
            hb[3] - hb[1] > rb[3] - rb[1],
            "longer text, taller when turned: {hb:?}"
        );
        assert!(hb[2] <= 5.0 + 1e-9);
        assert_eq!(
            d("document.set_text", json!({"id": text, "text": "Hello"})).unwrap()["changed"],
            false
        );
        assert!(d("document.set_text", json!({"id": rect, "text": "x"})).is_err());
        assert!(d("document.set_text", json!({"id": text, "height_mm": 0})).is_err());
        assert_eq!(d("history.undo", Value::Null).unwrap()["step"], "edit text");
        assert_eq!(bounds(&text).1["text"]["text"], "Hi");
        d("history.redo", Value::Null).unwrap();

        // transforms
        d(
            "document.transform",
            json!({"ids": [rect], "op": {"translate": [1, 2]}}),
        )
        .unwrap();
        assert!(close(bounds(&rect).0, [1.0, 2.0, 11.0, 7.0]));
        d(
            "document.transform",
            json!({"ids": [rect], "op": {"scale": [2, -1], "pivot": [1, 2]}}),
        )
        .unwrap();
        assert!(close(bounds(&rect).0, [1.0, -3.0, 21.0, 2.0]));
        d(
            "document.transform",
            json!({"ids": [rect], "op": {"matrix": [1, 0, 0, 1, -1, 3]}}),
        )
        .unwrap();
        assert!(close(bounds(&rect).0, [0.0, 0.0, 20.0, 5.0]));
        assert!(
            d(
                "document.transform",
                json!({"op": {"scale": [0, 1], "pivot": [0, 0]}})
            )
            .is_err()
        );
        assert!(
            d(
                "document.transform",
                json!({"op": {"matrix": [1, 1, 1, 1, 0, 0]}})
            )
            .is_err()
        );
        assert!(d("document.transform", json!({"op": {"spin": 1}})).is_err());

        // align and duplicate
        d("document.align", json!({"ids": [rect, ell], "how": "right"})).unwrap();
        assert!(close(bounds(&rect).0, [10.0, 0.0, 30.0, 5.0]));
        d("document.align", json!({"ids": [rect, ell], "how": "top"})).unwrap();
        assert!(close(bounds(&rect).0, [10.0, 5.0, 30.0, 10.0]));
        d("document.align", json!({"ids": [rect, ell], "how": "center_h"})).unwrap();
        d("document.align", json!({"ids": [rect, ell], "how": "middle_v"})).unwrap();
        assert!(close(bounds(&rect).0, [10.0, 2.5, 30.0, 7.5]));
        assert!(close(bounds(&ell).0, [15.0, 0.0, 25.0, 10.0]));
        d("document.align", json!({"ids": [rect, ell], "how": "left"})).unwrap();
        d("document.align", json!({"ids": [rect, ell], "how": "bottom"})).unwrap();
        assert!(close(bounds(&ell).0, [10.0, 0.0, 20.0, 10.0]));
        assert_eq!(
            d("document.align", json!({"ids": [rect], "how": "left"})).unwrap()["changed"],
            false
        );
        let copies = d("document.duplicate", json!({"ids": [text, ell]})).unwrap()["ids"].clone();
        let copies: Vec<u64> = serde_json::from_value(copies).unwrap();
        assert_eq!(copies.len(), 2);
        let snap = d("session.snapshot", Value::Null).unwrap();
        assert_eq!(snap["selection"], json!(copies));
        let (cb, co) = bounds(&json!(copies[1]));
        assert_eq!(co["text"]["text"], "Hello");
        assert!(close(cb, [hb[0] + 5.0, hb[1] - 5.0, hb[2] + 5.0, hb[3] - 5.0]));
        d("document.set_text", json!({"id": copies[1], "text": "Hey"})).unwrap();
        assert_eq!(
            bounds(&text).1["text"]["text"],
            "Hello",
            "the original keeps its text"
        );

        // layer order
        let layers = |d: &dyn Fn(&str, Value) -> Result<Value, ActionError>| -> Vec<Value> {
            d("session.snapshot", Value::Null).unwrap()["document"]["layers"]
                .as_array()
                .unwrap()
                .iter()
                .map(|l| l["id"].clone())
                .collect()
        };
        let before = layers(&d);
        d("document.move_layer", json!({"id": before[0], "to": 9})).unwrap();
        assert_eq!(layers(&d), vec![before[1].clone(), before[0].clone()]);
        assert_eq!(
            d("document.move_layer", json!({"id": before[0], "to": 1})).unwrap()["changed"],
            false
        );
        assert!(d("document.move_layer", json!({"id": 99, "to": 0})).is_err());

        // an import on the current layer, and the geometry revision
        let rev = d("session.snapshot", Value::Null).unwrap()["document_revision"].clone();
        d("document.select", json!({"ids": []})).unwrap();
        assert_eq!(
            d("session.snapshot", Value::Null).unwrap()["document_revision"],
            rev
        );
        let imported = d(
            "document.import_data",
            json!({"format": "svg", "text": SVG, "colors": "unify"}),
        )
        .unwrap();
        assert_eq!(imported["layers"], json!([]));
        let snap = d("session.snapshot", Value::Null).unwrap();
        assert_ne!(snap["document_revision"], rev);
        let on_current = snap["document"]["objects"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|o| o["layer"] == first)
            .count();
        assert_eq!(
            on_current, 5,
            "rectangle, ellipse and its copy, and the two imported paths"
        );
        assert!(snap["document"]["layers"][0]["fill"]["lines_per_mm"].is_number());

        // toggle selection
        d("document.select", json!({"ids": [rect]})).unwrap();
        assert_eq!(
            d("document.select", json!({"ids": [rect, ell], "mode": "toggle"})).unwrap(),
            json!([ell])
        );
    }

    #[test]
    fn combine_and_split() {
        let (r, h) = setup();
        let user = Caller::user();
        let d = |name: &str, p: Value| r.dispatch(&h, &user, name, p);
        let objects = || d("session.snapshot", Value::Null).unwrap()["document"]["objects"].clone();
        // a square drawn as four loose lines (like a DXF), and a circle elsewhere
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="40mm" height="20mm" viewBox="0 0 40 20">
            <line x1="0" y1="0" x2="10" y2="0" stroke="black"/><line x1="10" y1="10" x2="10" y2="0" stroke="black"/>
            <line x1="0" y1="10" x2="10" y2="10" stroke="black"/><line x1="0" y1="0" x2="0" y2="10" stroke="black"/>
            <circle cx="30" cy="5" r="3" stroke="red" fill="none"/></svg>"##;
        let ids = d(
            "document.import_data",
            json!({"format": "svg", "text": svg, "name": "s.svg"}),
        )
        .unwrap()["objects"]
            .clone();
        assert_eq!(ids.as_array().unwrap().len(), 5);
        let undo = d("session.snapshot", Value::Null).unwrap()["undo"]
            .as_array()
            .unwrap()
            .len();
        // nothing to join on its own
        assert_eq!(
            d("document.combine", json!({"ids": [ids[4]]})).unwrap(),
            json!({"ids": []})
        );
        // the lines become one closed square; the circle stays on its own layer
        let joined = d("document.combine", json!({"ids": ids})).unwrap()["ids"].clone();
        assert_eq!(joined.as_array().unwrap().len(), 1);
        assert_eq!(objects().as_array().unwrap().len(), 2);
        let snap = d("session.snapshot", Value::Null).unwrap();
        assert_eq!(snap["selection"], joined);
        assert_eq!(snap["undo"].as_array().unwrap().len(), undo + 1);
        let doc = d("document.get", Value::Null).unwrap();
        let square = doc["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["id"] == joined[0])
            .cloned()
            .unwrap();
        assert_eq!(square["paths"].as_array().unwrap().len(), 1);
        assert_eq!(square["paths"][0]["closed"], true);
        assert_eq!(square["source"]["format"], "svg");
        // split: one object per path (an object with one path is left as it is)
        let a = d(
            "document.add_shape",
            json!({"shape": "rect", "x": 50, "y": 0, "width": 5, "height": 5}),
        )
        .unwrap()["id"]
            .clone();
        let b = d(
            "document.add_shape",
            json!({"shape": "rect", "x": 60, "y": 0, "width": 5, "height": 5}),
        )
        .unwrap()["id"]
            .clone();
        let both = d("document.combine", json!({"ids": [a, b]})).unwrap()["ids"].clone();
        assert_eq!(both.as_array().unwrap().len(), 1);
        assert_eq!(objects().as_array().unwrap().len(), 3);
        let pieces = d("document.split", json!({"ids": [both[0], joined[0]]})).unwrap()["ids"].clone();
        assert_eq!(pieces.as_array().unwrap().len(), 2);
        assert_eq!(objects().as_array().unwrap().len(), 4);
        assert_eq!(d("history.undo", Value::Null).unwrap(), json!({"step": "split"}));
        assert_eq!(objects().as_array().unwrap().len(), 3);
        // texts are left as they are
        let t = d("document.add_text", json!({"text": "Hi", "x": 0, "y": 30})).unwrap()["id"].clone();
        assert_eq!(
            d("document.split", json!({"ids": [t]})).unwrap(),
            json!({"ids": []})
        );
        let only = d("document.combine", json!({"ids": [t, both[0]]})).unwrap();
        assert_eq!(only, json!({"ids": []}));
    }

    #[test]
    fn clipboard_actions() {
        let (r, h) = setup();
        let user = Caller::user();
        let d = |name: &str, p: Value| r.dispatch(&h, &user, name, p);
        let snap = || d("session.snapshot", Value::Null).unwrap();
        let bounds = |id: &Value| -> [f64; 4] {
            let o = snap()["document"]["objects"]
                .as_array()
                .unwrap()
                .iter()
                .find(|o| o["id"] == *id)
                .cloned()
                .unwrap();
            serde_json::from_value(o["bounds"].clone()).unwrap()
        };
        let close = |a: [f64; 4], b: [f64; 4]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6);
        let count = || snap()["document"]["objects"].as_array().unwrap().len();

        // nothing selected: nothing copied, nothing pasted
        assert_eq!(d("document.copy", Value::Null).unwrap(), json!({"objects": 0}));
        assert_eq!(d("document.paste", Value::Null).unwrap(), json!({"ids": []}));
        assert_eq!(d("document.cut", Value::Null).unwrap(), json!({"objects": 0}));
        let a = d(
            "document.add_shape",
            json!({"shape": "rect", "x": 0, "y": 0, "width": 10, "height": 10}),
        )
        .unwrap()["id"]
            .clone();
        let red = d(
            "document.add_layer",
            json!({"color": "#FF0000", "speed_mm_min": 321, "power": 77}),
        )
        .unwrap()["id"]
            .clone();
        let t = d(
            "document.add_text",
            json!({"text": "Hi", "x": 20, "y": 0, "height_mm": 5, "layer": red}),
        )
        .unwrap()["id"]
            .clone();
        d("document.select", json!({"ids": [a, t]})).unwrap();
        let undo = snap()["undo"].as_array().unwrap().len();
        assert_eq!(d("document.copy", Value::Null).unwrap(), json!({"objects": 2}));
        // copying is not an undo step
        assert_eq!(snap()["undo"].as_array().unwrap().len(), undo);
        assert_eq!(snap()["clipboard"], 2);

        // same document: every paste is offset once more, and selected
        let p1 = d("document.paste", Value::Null).unwrap()["ids"].clone();
        assert_eq!(p1.as_array().unwrap().len(), 2);
        assert_eq!(snap()["selection"], p1);
        assert!(close(bounds(&p1[0]), [5.0, -5.0, 15.0, 5.0]));
        let p2 = d("document.paste", Value::Null).unwrap()["ids"].clone();
        assert!(close(bounds(&p2[0]), [10.0, -10.0, 20.0, 0.0]));
        let text = snap()["document"]["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["id"] == p2[1])
            .cloned()
            .unwrap();
        assert_eq!(text["text"]["text"], "Hi");
        assert_eq!(text["layer"], red);
        assert_eq!(d("history.undo", Value::Null).unwrap(), json!({"step": "paste"}));
        let p3 = d("document.paste", json!({"offset_mm": [0, 30]})).unwrap()["ids"].clone();
        assert!(close(bounds(&p3[0]), [0.0, 30.0, 10.0, 40.0]));
        // centred where the context menu opened: the rect and the text together
        let all = d("document.paste", json!({"at_mm": [100, 100]})).unwrap()["ids"].clone();
        let (r0, t0) = (bounds(&all[0]), bounds(&all[1]));
        let (cx, cy) = ((r0[0] + t0[2]) / 2.0, (r0[1].min(t0[1]) + r0[3].max(t0[3])) / 2.0);
        assert!(
            (cx - 100.0).abs() < 1e-6 && (cy - 100.0).abs() < 1e-6,
            "{cx} {cy}"
        );
        assert!(d("document.paste", json!({"at_mm": [f64::NAN, 0]})).is_err());
        assert!(d("document.paste", json!({"offset_mm": [f64::MAX, 0]})).is_ok());
        assert!(d("document.paste", json!({"offset_mm": [0, "x"]})).is_err());

        // cut: deleted (one undo step), pasted back in place
        let n = count();
        d("document.select", json!({"ids": [a]})).unwrap();
        assert_eq!(d("document.cut", Value::Null).unwrap(), json!({"objects": 1}));
        assert_eq!(count(), n - 1);
        assert_eq!(snap()["undo"].as_array().unwrap().last().unwrap(), "cut");
        let back = d("document.paste", Value::Null).unwrap()["ids"].clone();
        assert!(close(bounds(&back[0]), [0.0, 0.0, 10.0, 10.0]));

        // another document: the clipboard survives; first paste in place, the layer
        // is made again with the copied settings
        d("document.select", json!({"ids": [t]})).unwrap();
        d("document.copy", Value::Null).unwrap();
        d("document.new", Value::Null).unwrap();
        assert_eq!(snap()["clipboard"], 1);
        let q = d("document.paste", Value::Null).unwrap()["ids"].clone();
        let layers = snap()["document"]["layers"].clone();
        assert_eq!(layers.as_array().unwrap().len(), 1);
        assert_eq!(layers[0]["color"], "#FF0000");
        assert_eq!(
            (layers[0]["speed_mm_min"].clone(), layers[0]["power"].clone()),
            (json!(321), json!(77))
        );
        let qb = bounds(&q[0]);
        assert!(qb[0] >= 20.0 - 1e-9 && qb[1] >= -1e-9, "{qb:?}");
        // a second paste there is offset
        let q2 = d("document.paste", Value::Null).unwrap()["ids"].clone();
        let q2b = bounds(&q2[0]);
        assert!((q2b[0] - qb[0] - 5.0).abs() < 1e-6);
        // a black layer in the new document takes black objects
        d("document.add_layer", json!({"color": "#000000"})).unwrap();
        let blacks = snap()["document"]["layers"].as_array().unwrap().len();
        d("document.select", json!({"ids": q})).unwrap();
        d("document.cut", Value::Null).unwrap();
        d("document.paste", Value::Null).unwrap();
        assert_eq!(snap()["document"]["layers"].as_array().unwrap().len(), blacks);
    }

    #[test]
    fn document_actions() {
        let (r, h) = setup();
        let user = Caller::user();
        let d = |name: &str, p: Value| r.dispatch(&h, &user, name, p);
        let ids = d(
            "document.import_data",
            json!({"format": "svg", "data": b64(SVG.as_bytes()), "name": "a.svg"}),
        )
        .unwrap();
        assert_eq!(ids["layers"].as_array().unwrap().len(), 2);
        assert_eq!(ids["unsupported"], 0);
        let ids: Vec<u64> = serde_json::from_value(ids["objects"].clone()).unwrap();
        assert_eq!(ids.len(), 2);
        let snap = d("session.snapshot", Value::Null).unwrap();
        assert_eq!(snap["document"]["layers"].as_array().unwrap().len(), 2);
        assert_eq!(snap["undo"], json!(["import"]));

        assert_eq!(
            d("document.select", json!({"ids": [ids[0], 999]})).unwrap(),
            json!([ids[0]])
        );
        assert_eq!(
            d("document.select", json!({"ids": [ids[1]], "mode": "add"}))
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            d("document.select", json!({"ids": [ids[1]], "mode": "remove"})).unwrap(),
            json!([ids[0]])
        );
        assert_eq!(d("document.select_all", Value::Null).unwrap(), json!(2));
        d("document.clear_selection", Value::Null).unwrap();
        d("document.select", json!({"ids": [ids[0]]})).unwrap();
        // move the selection
        let before = d("session.snapshot", Value::Null).unwrap()["document"]["objects"][0]["bounds"].clone();
        assert_eq!(
            d("document.translate", json!({"dx": 10.0, "dy": 0.0})).unwrap(),
            json!({"changed": true})
        );
        let after = d("session.snapshot", Value::Null).unwrap()["document"]["objects"][0]["bounds"].clone();
        assert_eq!(after[0].as_f64().unwrap(), before[0].as_f64().unwrap() + 10.0);
        assert!(d("document.translate", json!({"dx": f64::NAN, "dy": 0.0})).is_err());
        // layers
        let lid = d(
            "document.add_layer",
            json!({"color": "#00FF00", "mode": "cut", "power": 500}),
        )
        .unwrap()["id"]
            .clone();
        assert!(d("document.add_layer", json!({"color": "#00FF00", "passes": 0})).is_err());
        assert_eq!(
            d("document.move_to_layer", json!({"layer": lid})).unwrap(),
            json!({"changed": true})
        );
        assert!(d("document.move_to_layer", json!({"layer": 77})).is_err());
        assert_eq!(
            d("document.update_layer", json!({"id": lid, "speed_mm_min": 300, "lines_per_mm": 5.0, "visible": false, "name": "x", "fill_pattern": "cross"}))
                .unwrap(),
            json!({"changed": true})
        );
        assert_eq!(
            d("document.update_layer", json!({"id": lid, "speed_mm_min": 300})).unwrap(),
            json!({"changed": false})
        );
        // M3 / M4 per layer
        d(
            "document.update_layer",
            json!({"id": lid, "laser_mode": "dynamic"}),
        )
        .unwrap();
        let layer = d("session.snapshot", Value::Null).unwrap()["document"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["id"] == lid)
            .cloned()
            .unwrap();
        assert_eq!(layer["laser_mode"], "dynamic");
        assert!(d("document.update_layer", json!({"id": lid, "laser_mode": "m5"})).is_err());
        assert!(d("document.update_layer", json!({"id": 77})).is_err());
        assert!(d("document.update_layer", json!({"id": lid, "lines_per_mm": -1.0})).is_err());
        assert!(
            d("document.remove_layer", json!({"id": 77})).is_err(),
            "unknown layer"
        );
        // delete, undo, redo
        assert_eq!(
            d("document.delete", json!({"ids": [ids[1]]})).unwrap(),
            json!({"changed": true})
        );
        assert_eq!(
            d("document.delete", json!({"ids": [ids[1]]})).unwrap(),
            json!({"changed": false})
        );
        assert_eq!(d("history.undo", Value::Null).unwrap(), json!({"step": "delete"}));
        assert_eq!(d("history.redo", Value::Null).unwrap(), json!({"step": "delete"}));
        // fkproj round trip
        let text = d("document.to_fkproj", Value::Null).unwrap()["text"]
            .as_str()
            .unwrap()
            .to_owned();
        d("document.new", Value::Null).unwrap();
        assert_eq!(d("document.get", Value::Null).unwrap()["objects"], json!([]));
        d("document.from_fkproj", json!({"text": text})).unwrap();
        assert_eq!(
            d("document.get", Value::Null).unwrap()["objects"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(d("document.from_fkproj", json!({"text": "{}"})).is_err());
        assert!(d("document.import_data", json!({"format": "svg", "data": "!!"})).is_err());
        assert!(d("document.import_data", json!({"format": "svg"})).is_err());
        assert!(d("document.import_data", json!({"format": "image", "text": "x"})).is_err());
        assert!(
            d(
                "document.import_data",
                json!({"format": "svg", "text": "x", "data": "eA=="})
            )
            .is_err()
        );
        let text = d("document.import_data", json!({"format": "svg", "text": SVG})).unwrap();
        assert_eq!(text["objects"].as_array().unwrap().len(), 2);
        d("history.undo", Value::Null).unwrap();
        assert!(
            d(
                "document.import_data",
                json!({"format": "dxf", "data": b64(b"junk")})
            )
            .is_err()
        );
        // a layer goes with its objects, in one undoable step
        let n = |d: &dyn Fn(&str, Value) -> Result<Value, ActionError>| {
            d("document.get", Value::Null).unwrap()["objects"]
                .as_array()
                .unwrap()
                .len()
        };
        let layer = d("document.get", Value::Null).unwrap()["objects"][0]["layer"].clone();
        d("document.remove_layer", json!({"id": layer})).unwrap();
        assert_eq!(n(&d), 0);
        d("history.undo", Value::Null).unwrap();
        assert_eq!(n(&d), 1);
    }

    #[test]
    fn importing_files_by_extension() {
        let mut s = Session::new();
        assert_eq!(import_file(&mut s, "dir/a.SVG", SVG.as_bytes()).unwrap().len(), 2);
        assert_eq!(s.document().objects[0].source.as_ref().unwrap().file, "a.SVG");
        assert!(import_file(&mut s, "a.nc", b"G0").is_err());
        assert!(import_file(&mut s, "noext", b"").is_err());
    }

    #[test]
    fn images_and_dxf() {
        let (r, h) = setup();
        let user = Caller::user();
        let png = freekerf_raster::image::Image::new(96, 48, 0xFF00_0000)
            .encode_png()
            .unwrap();
        let out = r
            .dispatch(
                &h,
                &user,
                "document.import_data",
                json!({"format": "image", "data": b64(&png), "name": "p.png"}),
            )
            .unwrap();
        assert_eq!(out["objects"].as_array().unwrap().len(), 1);
        let doc = r.dispatch(&h, &user, "document.get", Value::Null).unwrap();
        let size = &doc["objects"][0]["size_mm"];
        assert!((size[0].as_f64().unwrap() - 25.4).abs() < 1e-9);
        let dxf = "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n5\n21\n5\n0\nENDSEC\n0\nEOF\n";
        let out = r
            .dispatch(
                &h,
                &user,
                "document.import_data",
                json!({"format": "dxf", "data": b64(dxf.as_bytes())}),
            )
            .unwrap();
        assert_eq!(out["objects"].as_array().unwrap().len(), 1);
        assert!(
            r.dispatch(
                &h,
                &user,
                "document.import_data",
                json!({"format": "image", "data": b64(b"x")})
            )
            .is_err()
        );
    }

    #[test]
    fn view_actions_and_tiers() {
        let (r, h) = setup();
        let user = Caller::user();
        let d = |name: &str, p: Value| r.dispatch(&h, &user, name, p);
        d("view.set_mode", json!({"mode": "design"})).unwrap();
        d("view.set_tool", json!({"tool": "rect"})).unwrap();
        d("view.open_panel", json!({"name": "console"})).unwrap();
        d("view.close_panel", json!({"name": "console"})).unwrap();
        d("view.open_panel", json!({"name": "layers"})).unwrap();
        assert!(d("view.open_panel", json!({"name": "nope"})).is_err());
        d("view.set_viewport", json!({"center": [10.0, 20.0], "zoom": 4.0})).unwrap();
        assert!(d("view.set_viewport", json!({"center": [10.0, 20.0], "zoom": 0.0})).is_err());
        d("view.open_dialog", json!({"name": "raster"})).unwrap();
        d("view.open_dialog", json!({"name": "settings"})).unwrap();
        d("view.close_dialog", Value::Null).unwrap();
        let id = d("view.toast", json!({"message": "toast.saved", "level": "ok"})).unwrap();
        assert!(d("view.toast", json!({"message": " "})).is_err());
        d("view.fit", Value::Null).unwrap();
        d("view.focus", json!({"target": "canvas"})).unwrap();
        let v = d("view.get", Value::Null).unwrap();
        assert_eq!(v["mode"], "design");
        assert_eq!(v["tool"], "rect");
        assert_eq!(
            (v["panel"].clone(), v["drawer_open"].clone()),
            (json!("layers"), json!(true))
        );
        assert_eq!(v["dialogs"], json!(["raster"]));
        assert_eq!(v["viewport"], json!({"center": [10.0, 20.0], "zoom": 4.0}));
        assert_eq!(v["toasts"][0]["id"], id);
        assert_eq!(
            (v["fit_request"].clone(), v["focus"].clone()),
            (json!(1), json!("canvas"))
        );
        d("view.dismiss_toast", json!({"id": id})).unwrap();
        d("view.close_dialog", json!({"name": "raster"})).unwrap();
        // every view change is a session event
        assert!(
            h.0.lock()
                .unwrap()
                .drain_events()
                .iter()
                .any(|e| e.change == crate::session::Change::View)
        );
        // a read-only caller cannot select or edit
        let reader = Caller::new(crate::CallerKind::Agent, "a", Tier::Read);
        assert!(r.dispatch(&h, &reader, "session.snapshot", Value::Null).is_ok());
        assert!(matches!(
            r.dispatch(&h, &reader, "document.select_all", Value::Null),
            Err(ActionError::PermissionDenied { .. })
        ));
        // every session action has a description and a tier; names are unique
        assert!(r.specs().all(|s| !s.description.is_empty()));
        assert!(r.len() >= 24);
        let mut again = Registry::<Host>::new();
        register_session_actions(&mut again).unwrap();
        assert!(register_session_actions(&mut again).is_err());
    }
}
