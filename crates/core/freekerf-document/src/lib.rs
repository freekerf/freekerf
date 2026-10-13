//! The FreeKerf project: objects on colour layers, each layer with its cut settings,
//! and the versioned `.fkproj` file format.
//!
//! A [`Document`] is pure data (core layer, no I/O): importers fill it, the
//! session edits it through actions, the planner turns it into a job. The file is
//! JSON (`format = "freekerf-project"`, `version = 1`); its JSON schema is
//! generated from this model ([`json_schema`], committed as
//! `doc/schema/fkproj.schema.json` and checked in CI). Older versions are migrated
//! when read, newer ones are refused.
//!
//! ```
//! use freekerf_document::{Color, Document, ObjectKind};
//! use freekerf_geometry::{Path, Point};
//!
//! let mut doc = Document::new();
//! let layer = doc.layer_for_color(Color::BLACK, 1000, 1000);
//! let square = Path::polyline(&[Point::ZERO, Point::new(10.0, 0.0), Point::new(10.0, 10.0)], true);
//! doc.add_object(layer, ObjectKind::Shape { paths: vec![square] }).unwrap();
//! let bytes = doc.to_fkproj();
//! assert_eq!(Document::from_fkproj(bytes.as_bytes()).unwrap(), doc);
//! ```

mod image;
mod layer;
mod object;

use freekerf_geometry::Rect;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use image::{
    Adjustments, Bytes, CenterlineSettings, DitherMode, GrayFormula, ImageData, ImageEdit, ImageObject,
    Interpolation, LineDirection, RasterSettings, RasterTool, VectorFill, VectorizeSettings,
};
pub use layer::{Color, FillSettings, LaserMode, Layer, LayerId, LayerMode};
pub use object::{
    Object, ObjectId, ObjectKind, Source, SourceFormat, TextSpec, UnsupportedReason, ellipse_path, rect_path,
};

/// Value of the `format` field of a project file.
pub const FORMAT: &str = "freekerf-project";
/// Current version of the project file.
pub const VERSION: u32 = 3;

/// Errors of documents and project files.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DocumentError {
    /// Not a project file, or not matching the model.
    #[error("invalid project: {0}")]
    Invalid(String),
    /// Written by a newer FreeKerf.
    #[error("project version {found} is newer than this FreeKerf supports ({supported})")]
    TooNew {
        /// Version in the file.
        found: u32,
        /// Newest supported version.
        supported: u32,
    },
    /// No such layer.
    #[error("no layer {0}")]
    NoLayer(u32),
    /// No such object.
    #[error("no object {0}")]
    NoObject(u64),
}

/// A project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Document {
    /// Always [`FORMAT`].
    pub format: String,
    /// File version ([`VERSION`] when written by this FreeKerf).
    pub version: u32,
    /// Layers in user order (execution order also depends on their mode).
    pub layers: Vec<Layer>,
    /// Objects in drawing order.
    pub objects: Vec<Object>,
    /// Next object id (ids are never reused).
    #[serde(default)]
    pub next_object_id: u64,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    /// An empty project.
    #[must_use]
    pub fn new() -> Self {
        Self {
            format: FORMAT.into(),
            version: VERSION,
            layers: Vec::new(),
            objects: Vec::new(),
            next_object_id: 1,
        }
    }

    /// The layer with this id.
    #[must_use]
    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    /// The layer with this id, mutable.
    pub fn layer_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.id == id)
    }

    /// The first layer of this colour.
    #[must_use]
    pub fn layer_by_color(&self, color: Color) -> Option<&Layer> {
        self.layers.iter().find(|l| l.color == color)
    }

    /// The layer of this colour, created (with `speed`/`power`) when missing.
    pub fn layer_for_color(&mut self, color: Color, speed_mm_min: u32, power: u32) -> LayerId {
        if let Some(l) = self.layer_by_color(color) {
            return l.id;
        }
        self.add_layer(Layer::new(LayerId(0), color, speed_mm_min, power))
    }

    /// Adds a layer (its id is assigned) and returns its id.
    pub fn add_layer(&mut self, mut layer: Layer) -> LayerId {
        let id = LayerId(self.layers.iter().map(|l| l.id.0).max().map_or(1, |m| m + 1));
        layer.id = id;
        self.layers.push(layer);
        id
    }

    /// Removes a layer and its objects.
    ///
    /// # Errors
    /// Unknown layer.
    pub fn remove_layer(&mut self, id: LayerId) -> Result<Layer, DocumentError> {
        let pos = self
            .layers
            .iter()
            .position(|l| l.id == id)
            .ok_or(DocumentError::NoLayer(id.0))?;
        self.objects.retain(|o| o.layer != id);
        Ok(self.layers.remove(pos))
    }

    /// Adds an object to a layer and returns its id.
    ///
    /// # Errors
    /// Unknown layer.
    pub fn add_object(&mut self, layer: LayerId, kind: ObjectKind) -> Result<ObjectId, DocumentError> {
        self.add_object_with(layer, kind, String::new(), None)
    }

    /// Adds a named object with its source.
    ///
    /// # Errors
    /// Unknown layer.
    pub fn add_object_with(
        &mut self,
        layer: LayerId,
        kind: ObjectKind,
        name: String,
        source: Option<Source>,
    ) -> Result<ObjectId, DocumentError> {
        if self.layer(layer).is_none() {
            return Err(DocumentError::NoLayer(layer.0));
        }
        let id = ObjectId(self.next_object_id.max(1));
        self.next_object_id = id.0 + 1;
        self.objects.push(Object {
            id,
            layer,
            name,
            source,
            text: None,
            transform: freekerf_geometry::Matrix::IDENTITY,
            kind,
        });
        Ok(id)
    }

    /// The object with this id.
    #[must_use]
    pub fn object(&self, id: ObjectId) -> Option<&Object> {
        self.objects.iter().find(|o| o.id == id)
    }

    /// The object with this id, mutable.
    pub fn object_mut(&mut self, id: ObjectId) -> Option<&mut Object> {
        self.objects.iter_mut().find(|o| o.id == id)
    }

    /// Removes an object.
    ///
    /// # Errors
    /// Unknown object.
    pub fn remove_object(&mut self, id: ObjectId) -> Result<Object, DocumentError> {
        let pos = self
            .objects
            .iter()
            .position(|o| o.id == id)
            .ok_or(DocumentError::NoObject(id.0))?;
        Ok(self.objects.remove(pos))
    }

    /// Moves an object to another layer.
    ///
    /// # Errors
    /// Unknown object or layer.
    pub fn set_object_layer(&mut self, id: ObjectId, layer: LayerId) -> Result<(), DocumentError> {
        if self.layer(layer).is_none() {
            return Err(DocumentError::NoLayer(layer.0));
        }
        self.object_mut(id).ok_or(DocumentError::NoObject(id.0))?.layer = layer;
        Ok(())
    }

    /// Objects of a layer, in drawing order.
    pub fn objects_on(&self, layer: LayerId) -> impl Iterator<Item = &Object> {
        self.objects.iter().filter(move |o| o.layer == layer)
    }

    /// Layers in execution order: fillings first, then lines, cuts last; user
    /// order inside each group; ignored layers left out (a stable sort, as LaserGRBL).
    #[must_use]
    pub fn execution_order(&self) -> Vec<&Layer> {
        let mut v: Vec<&Layer> = self
            .layers
            .iter()
            .filter(|l| l.mode != LayerMode::Ignore)
            .collect();
        v.sort_by_key(|l| l.mode.execution_order());
        v
    }

    /// Bounds of every object.
    #[must_use]
    pub fn bounds(&self) -> Rect {
        let mut r = Rect::EMPTY;
        for o in &self.objects {
            r.union(&o.bounds());
        }
        r
    }

    /// Checks the invariants: format name, supported version, unique layer and
    /// object ids, every object on an existing layer, `passes >= 1`.
    ///
    /// # Errors
    /// The first broken invariant.
    pub fn validate(&self) -> Result<(), DocumentError> {
        if self.format != FORMAT {
            return Err(DocumentError::Invalid(format!(
                "format is `{}`, not `{FORMAT}`",
                self.format
            )));
        }
        let mut ids: Vec<u32> = self.layers.iter().map(|l| l.id.0).collect();
        ids.sort_unstable();
        if ids.windows(2).any(|w| w[0] == w[1]) {
            return Err(DocumentError::Invalid("duplicate layer id".into()));
        }
        if let Some(l) = self.layers.iter().find(|l| l.passes == 0) {
            return Err(DocumentError::Invalid(format!("layer {} has 0 passes", l.id.0)));
        }
        let mut oids: Vec<u64> = self.objects.iter().map(|o| o.id.0).collect();
        oids.sort_unstable();
        if oids.windows(2).any(|w| w[0] == w[1]) {
            return Err(DocumentError::Invalid("duplicate object id".into()));
        }
        if let Some(o) = self.objects.iter().find(|o| self.layer(o.layer).is_none()) {
            return Err(DocumentError::Invalid(format!(
                "object {} is on missing layer {}",
                o.id.0, o.layer.0
            )));
        }
        Ok(())
    }

    /// The project file (pretty JSON, stable field order, ends with a newline).
    #[must_use]
    pub fn to_fkproj(&self) -> String {
        let mut s = serde_json::to_string_pretty(self).unwrap_or_default();
        s.push('\n');
        s
    }

    /// Reads a project file, migrating older versions.
    ///
    /// # Errors
    /// Not a project, newer version, broken invariants.
    pub fn from_fkproj(bytes: &[u8]) -> Result<Self, DocumentError> {
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|e| DocumentError::Invalid(e.to_string()))?;
        if value.get("format").and_then(serde_json::Value::as_str) != Some(FORMAT) {
            return Err(DocumentError::Invalid(format!("not a `{FORMAT}` file")));
        }
        let found = value
            .get("version")
            .and_then(serde_json::Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| DocumentError::Invalid("missing version".into()))?;
        if found > VERSION {
            return Err(DocumentError::TooNew {
                found,
                supported: VERSION,
            });
        }
        // migrations, step by step:
        // 1 -> 2: objects may carry `text` (editable text) and `transform`; both are
        //         optional, so nothing changes in `value`
        // 2 -> 3: layers may carry `laser_mode` (`machine` when absent), so nothing
        //         changes either; the version keeps older FreeKerf from planning a
        //         dynamic-power layer with the machine's command
        let mut doc: Self =
            serde_json::from_value(value).map_err(|e| DocumentError::Invalid(e.to_string()))?;
        doc.version = VERSION;
        let max_id = doc.objects.iter().map(|o| o.id.0).max().unwrap_or(0);
        doc.next_object_id = doc.next_object_id.max(max_id + 1);
        doc.validate()?;
        Ok(doc)
    }
}

/// The JSON schema of the project file (`doc/schema/fkproj.schema.json`).
#[must_use]
pub fn json_schema() -> String {
    let schema = schemars::schema_for!(Document);
    let mut s = serde_json::to_string_pretty(&schema).unwrap_or_default();
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use freekerf_geometry::{Matrix, Path, Point};

    use super::*;

    fn square() -> ObjectKind {
        ObjectKind::Shape {
            paths: vec![Path::polyline(
                &[
                    Point::ZERO,
                    Point::new(10.0, 0.0),
                    Point::new(10.0, 10.0),
                    Point::new(0.0, 10.0),
                ],
                true,
            )],
        }
    }

    #[test]
    fn layers_and_objects() {
        let mut d = Document::new();
        let black = d.layer_for_color(Color::BLACK, 1000, 500);
        assert_eq!(d.layer_for_color(Color::BLACK, 1, 1), black);
        let red = d.layer_for_color(Color::rgb(255, 0, 0), 300, 1000);
        assert_ne!(black, red);
        let a = d.add_object(black, square()).unwrap();
        let b = d
            .add_object_with(
                red,
                square(),
                "frame".into(),
                Some(Source {
                    format: SourceFormat::Svg,
                    file: "a.svg".into(),
                }),
            )
            .unwrap();
        assert_ne!(a, b);
        assert_eq!(d.objects_on(black).count(), 1);
        assert_eq!(
            d.add_object(LayerId(99), square()),
            Err(DocumentError::NoLayer(99))
        );
        d.set_object_layer(b, black).unwrap();
        assert_eq!(d.objects_on(black).count(), 2);
        assert_eq!(
            d.set_object_layer(b, LayerId(77)),
            Err(DocumentError::NoLayer(77))
        );
        assert_eq!(
            d.set_object_layer(ObjectId(77), black),
            Err(DocumentError::NoObject(77))
        );
        d.object_mut(a).unwrap().translate(5.0, 5.0);
        assert_eq!(d.object(a).unwrap().bounds().min_x, 5.0);
        assert_eq!(d.bounds().max_x, 15.0);
        d.remove_object(a).unwrap();
        assert_eq!(d.remove_object(a), Err(DocumentError::NoObject(a.0)));
        // ids are never reused
        let c = d.add_object(black, square()).unwrap();
        assert!(c.0 > b.0);
        d.layer_mut(red).unwrap().mode = LayerMode::Cut;
        d.remove_layer(black).unwrap();
        assert!(d.objects.is_empty());
        assert_eq!(d.remove_layer(black), Err(DocumentError::NoLayer(black.0)));
    }

    #[test]
    fn execution_order_is_fill_line_cut_and_stable() {
        let mut d = Document::new();
        let modes = [
            LayerMode::Cut,
            LayerMode::Line,
            LayerMode::Fill,
            LayerMode::Ignore,
            LayerMode::Line,
        ];
        for (i, m) in modes.iter().enumerate() {
            let id = d.layer_for_color(Color::rgb(u8::try_from(i).unwrap(), 0, 0), 1000, 1000);
            d.layer_mut(id).unwrap().mode = *m;
        }
        let order: Vec<u8> = d.execution_order().iter().map(|l| l.color.r).collect();
        assert_eq!(order, vec![2, 1, 4, 0]);
    }

    #[test]
    fn project_files_round_trip_and_are_checked() {
        let mut d = Document::new();
        let l = d.layer_for_color(Color::BLACK, 1000, 500);
        d.add_object(l, square()).unwrap();
        d.add_object(
            l,
            ObjectKind::Unsupported {
                element: "text".into(),
                reason: UnsupportedReason::NotConverted,
            },
        )
        .unwrap();
        d.add_object(
            l,
            ObjectKind::Image(Box::new(ImageObject {
                data: ImageData::Rgba {
                    width: 1,
                    height: 1,
                    pixels: Bytes(vec![0, 0, 0, 255]),
                },
                position: Point::new(1.0, 2.0),
                size_mm: [10.0, 10.0],
                settings: RasterSettings::default(),
                edits: vec![ImageEdit::Invert],
            })),
        )
        .unwrap();
        let text = d.to_fkproj();
        assert!(text.ends_with('\n'));
        assert_eq!(Document::from_fkproj(text.as_bytes()).unwrap(), d);
        assert_eq!(d.objects[2].bounds().max_y, 12.0);
        assert!(d.objects[1].bounds().is_empty());

        let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
        v["version"] = (VERSION + 1).into();
        assert_eq!(
            Document::from_fkproj(v.to_string().as_bytes()),
            Err(DocumentError::TooNew {
                found: VERSION + 1,
                supported: VERSION
            })
        );
        v["version"] = serde_json::Value::Null;
        assert!(Document::from_fkproj(v.to_string().as_bytes()).is_err());
        assert!(Document::from_fkproj(b"{}").is_err());
        assert!(Document::from_fkproj(b"not json").is_err());
        let mut broken = d.clone();
        broken.objects[0].layer = LayerId(42);
        assert!(Document::from_fkproj(broken.to_fkproj().as_bytes()).is_err());
        let mut dup = d.clone();
        dup.objects[1].id = dup.objects[0].id;
        assert!(dup.validate().is_err());
        let mut dup_layer = d.clone();
        dup_layer.layers.push(dup_layer.layers[0].clone());
        assert!(dup_layer.validate().is_err());
        let mut zero = d.clone();
        zero.layers[0].passes = 0;
        assert!(zero.validate().is_err());
        let mut fmt = d;
        fmt.format = "x".into();
        assert!(fmt.validate().is_err());
        // a missing next id is recomputed
        let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
        v.as_object_mut().unwrap().remove("next_object_id");
        assert_eq!(
            Document::from_fkproj(v.to_string().as_bytes())
                .unwrap()
                .next_object_id,
            4
        );
    }

    #[test]
    fn version_2_files_open_as_version_3_with_the_machine_laser_mode() {
        let v2 = r##"{"format": "freekerf-project", "version": 2,
            "layers": [{"id": 1, "color": "#000000", "speed_mm_min": 1000, "power": 1000}],
            "objects": [], "next_object_id": 1}"##;
        let d = Document::from_fkproj(v2.as_bytes()).unwrap();
        assert_eq!(d.version, VERSION);
        assert_eq!(d.layers[0].laser_mode, LaserMode::Machine);
        // the default is left out; a chosen mode is kept
        assert!(!d.to_fkproj().contains("laser_mode"));
        let mut m4 = d;
        m4.layers[0].laser_mode = LaserMode::Dynamic;
        let text = m4.to_fkproj();
        assert!(text.contains("\"laser_mode\": \"dynamic\""), "{text}");
        assert_eq!(Document::from_fkproj(text.as_bytes()).unwrap(), m4);
        assert_eq!(LaserMode::Dynamic.command(), Some("M4"));
        assert_eq!(LaserMode::Constant.command(), Some("M3"));
        assert_eq!(LaserMode::Machine.command(), None);
    }

    #[test]
    fn version_1_files_open_as_version_2() {
        let v1 = r##"{"format": "freekerf-project", "version": 1,
            "layers": [{"id": 1, "color": "#000000", "speed_mm_min": 1000, "power": 1000}],
            "objects": [{"id": 1, "layer": 1, "kind": "shape",
                "paths": [{"start": {"x": 0, "y": 0}, "segments": [{"kind": "line", "end": {"x": 1, "y": 0}}]}]}],
            "next_object_id": 2}"##;
        let d = Document::from_fkproj(v1.as_bytes()).unwrap();
        assert_eq!(d.version, VERSION);
        assert_eq!(d.objects[0].text, None);
        assert_eq!(d.objects[0].transform, Matrix::IDENTITY);
        // version 2 keeps text and transform, and leaves them out when unused
        let mut d2 = d.clone();
        d2.objects[0].text = Some(TextSpec {
            text: "Hi".into(),
            height_mm: 5.0,
        });
        d2.objects[0]
            .apply_transform(&Matrix::translation(3.0, 4.0))
            .unwrap();
        let text = d2.to_fkproj();
        assert!(text.contains("\"transform\"") && text.contains("\"height_mm\""));
        assert_eq!(Document::from_fkproj(text.as_bytes()).unwrap(), d2);
        assert!(!d.to_fkproj().contains("\"transform\""));
    }

    #[test]
    fn transforms_and_shapes() {
        let mut d = Document::new();
        let l = d.layer_for_color(Color::BLACK, 1000, 500);
        let id = d
            .add_object(
                l,
                ObjectKind::Shape {
                    paths: vec![rect_path(0.0, 0.0, 10.0, 5.0)],
                },
            )
            .unwrap();
        let o = d.object_mut(id).unwrap();
        let mut m = Matrix::IDENTITY;
        m.rotate_at(90.0, 0.0, 0.0);
        o.apply_transform(&m).unwrap();
        let b = o.bounds();
        assert!(
            (b.min_x + 5.0).abs() < 1e-9 && (b.max_y - 10.0).abs() < 1e-9,
            "{b:?}"
        );
        o.translate(5.0, 0.0);
        assert!((o.transform.m12 - 1.0).abs() < 1e-12 && (o.transform.offset_x - 5.0).abs() < 1e-12);
        let e = ellipse_path(0.0, 0.0, 20.0, 10.0).bounds();
        assert!(e.min_x.abs() < 1e-9 && (e.max_x - 20.0).abs() < 1e-9 && (e.max_y - 10.0).abs() < 1e-9);
        // text strokes are written again through the transform
        o.set_text_paths(
            TextSpec {
                text: "-".into(),
                height_mm: 2.0,
            },
            &[Path::polyline(
                &[Point::new(0.0, 0.0), Point::new(1.0, 0.0)],
                false,
            )],
        );
        let ObjectKind::Shape { paths } = &o.kind else {
            panic!()
        };
        assert!((paths[0].end().x - 5.0).abs() < 1e-9 && (paths[0].end().y - 1.0).abs() < 1e-9);
        // images move and scale, but do not rotate or mirror
        let img = d
            .add_object(
                l,
                ObjectKind::Image(Box::new(ImageObject {
                    data: ImageData::Rgba {
                        width: 1,
                        height: 1,
                        pixels: Bytes(vec![0, 0, 0, 255]),
                    },
                    position: Point::new(1.0, 2.0),
                    size_mm: [10.0, 10.0],
                    settings: RasterSettings::default(),
                    edits: vec![],
                })),
            )
            .unwrap();
        let o = d.object_mut(img).unwrap();
        o.apply_transform(&Matrix::new(2.0, 0.0, 0.0, 0.5, 1.0, 0.0))
            .unwrap();
        assert_eq!(o.bounds().max_x, 23.0);
        assert!(o.apply_transform(&m).is_err());
        assert!(o.apply_transform(&Matrix::scaling(-1.0, 1.0)).is_err());
    }

    #[test]
    fn schema_describes_the_format() {
        let s = json_schema();
        assert!(s.contains("\"Document\""));
        assert!(s.contains("next_object_id"));
        assert!(s.ends_with('\n'));
        assert_eq!(Document::default().version, VERSION);
    }
}
