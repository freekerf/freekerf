//! From a document to a job: layers in execution order, fillings, path order and
//! the G-code text; plus the test pattern generators and Hershey text.
//!
//! The G-code is written the way LaserGRBL writes it (same commands, same number
//! formatting, same order), so a job made from an imported file matches LaserGRBL
//! and the `conformance` goldens; intentional differences are listed in
//! `doc/divergences.md`. Part of the core layer: no I/O.
//!
//! ```
//! use freekerf_document::{Color, Document, ObjectKind};
//! use freekerf_geometry::{Path, Point};
//! use freekerf_planner::{plan, PlanOptions};
//!
//! let mut doc = Document::new();
//! let layer = doc.layer_for_color(Color::BLACK, 1000, 800);
//! let line = Path::polyline(&[Point::ZERO, Point::new(10.0, 0.0)], false);
//! doc.add_object(layer, ObjectKind::Shape { paths: vec![line] }).unwrap();
//! let job = plan(&doc, &PlanOptions::default()).unwrap();
//! assert_eq!(job.lines.first().map(String::as_str), Some("M3 S0"));
//! ```

pub mod fill;
pub mod generators;
pub mod image;
pub mod origin;
pub mod vector;
pub mod writer;

/// The Hershey text of the test generators (it lives in `freekerf-text`).
pub use freekerf_text::hershey;

use freekerf_document::{Document, ObjectKind};
use freekerf_gcode::{AnalysisOptions, Program};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use vector::VectorStyle;

/// Firmware family, for the few G-code differences between them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Firmware {
    /// Grbl, grblHAL and compatible.
    #[default]
    Grbl,
    /// Smoothieware: power from 0 to 1 on every move.
    Smoothie,
    /// Marlin.
    Marlin,
    /// VigoWork.
    VigoWork,
}

/// What the planner needs to know about the machine and the user's G-code
/// preferences (LaserGRBL settings of the same meaning in parentheses).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MachineOptions {
    /// Firmware (`Firmware Type`).
    pub firmware: Firmware,
    /// Power with `S` on a PWM laser; otherwise the laser is switched with the
    /// on/off commands (`Support Hardware PWM`).
    pub support_pwm: bool,
    /// Travel with `G1` instead of `G0` (`Disable G0 fast skip`).
    pub disable_g0: bool,
    /// The board's maximum `S` (`$30`), used without PWM.
    pub max_pwm: u32,
    /// Laser on command (`M3` constant, `M4` dynamic power).
    pub laser_on: String,
    /// Laser off command.
    pub laser_off: String,
    /// Feed of vector jobs with no layer settings, mm/min (border speed of the raster dialog).
    pub default_border_speed: u32,
    /// Power of vector jobs with no layer settings (maximum power of the raster dialog).
    pub default_power_max: u32,
    /// Raster lines all in the same direction (`Unidirectional Engraving`).
    pub unidirectional: bool,
}

impl Default for MachineOptions {
    fn default() -> Self {
        Self {
            firmware: Firmware::Grbl,
            support_pwm: true,
            disable_g0: false,
            max_pwm: 1000,
            laser_on: "M3".into(),
            laser_off: "M5".into(),
            default_border_speed: 1000,
            default_power_max: 255,
            unidirectional: false,
        }
    }
}

/// Options of [`plan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanOptions {
    /// Machine and G-code preferences.
    pub machine: MachineOptions,
    /// Writing style of vector jobs; by default from the objects' source.
    pub vector_style: Option<VectorStyle>,
    /// Write the per-layer comment and feed/power (colour layers). Off for jobs made
    /// without layers (LaserGRBL's centerline conversion).
    pub layer_preamble: bool,
}

impl Default for PlanOptions {
    fn default() -> Self {
        Self {
            machine: MachineOptions::default(),
            vector_style: None,
            layer_preamble: true,
        }
    }
}

/// Errors of planning.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    /// A geometry operation failed.
    #[error("geometry: {0}")]
    Geometry(String),
    /// The document has something this planner cannot write.
    #[error("not supported: {0}")]
    Unsupported(String),
    /// An image could not be processed.
    #[error("image: {0}")]
    Image(String),
}

impl From<freekerf_geometry::clipper::ClipperError> for PlanError {
    fn from(e: freekerf_geometry::clipper::ClipperError) -> Self {
        Self::Geometry(e.to_string())
    }
}

/// A job: G-code lines as written (comments included, like a saved file).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Job {
    /// Lines.
    pub lines: Vec<String>,
}

impl Job {
    /// The job loaded as a program (comments removed, blank lines skipped) and
    /// analyzed (time, ranges).
    #[must_use]
    pub fn program(&self, options: &AnalysisOptions) -> Program {
        let mut p = Program::new();
        p.append_text(&self.lines.join("\n"));
        p.analyze(options);
        p
    }

    /// The text of the job (`\n` line endings, final newline).
    #[must_use]
    pub fn text(&self) -> String {
        let mut s = self.lines.join("\n");
        s.push('\n');
        s
    }
}

/// Plans `doc`: vector objects on colour layers.
///
/// # Errors
/// Images (raster planning comes with `freekerf-raster`) or a failed filling.
pub fn plan(doc: &Document, options: &PlanOptions) -> Result<Job, PlanError> {
    plan_with(doc, options, None)
}

/// [`plan`] with a centerline tracer for images that use it. The vector objects
/// come first, then every image in document order, each as LaserGRBL's raster
/// dialog generates it ([`image::plan_image_object`]).
///
/// # Errors
/// As [`plan`], or an image that cannot be processed (centerline without a tracer).
pub fn plan_with(
    doc: &Document,
    options: &PlanOptions,
    tracer: Option<&dyn freekerf_raster::centerline::Tracer>,
) -> Result<Job, PlanError> {
    let has_vectors = doc
        .objects
        .iter()
        .any(|o| !matches!(o.kind, ObjectKind::Image(_)));
    let mut lines = if has_vectors || doc.objects.is_empty() {
        vector::plan_vectors(doc, options)?
    } else {
        Vec::new()
    };
    for o in &doc.objects {
        if let ObjectKind::Image(img) = &o.kind {
            // the layer's laser mode wins over the image's own command
            let mode = doc.layer(o.layer).and_then(|l| l.laser_mode.command());
            let lines_of = match mode {
                Some(cmd) if cmd != img.settings.laser_on => {
                    let mut img = img.clone();
                    cmd.clone_into(&mut img.settings.laser_on);
                    image::plan_image_object(&img, &options.machine, tracer)?
                }
                _ => image::plan_image_object(img, &options.machine, tracer)?,
            };
            lines.extend(lines_of);
        }
    }
    Ok(Job { lines })
}

#[cfg(test)]
mod tests {
    use freekerf_document::{Bytes, Color, ImageData, ImageObject, RasterSettings};
    use freekerf_geometry::Point;

    use super::*;

    #[test]
    fn jobs_load_as_programs() {
        let job = Job {
            lines: vec![
                "M3 S0".into(),
                "(comment)".into(),
                "S0 (pen)".into(),
                String::new(),
                "G1 X1 F100".into(),
            ],
        };
        let p = job.program(&AnalysisOptions::default());
        let texts: Vec<&str> = p.commands().map(freekerf_gcode::Command::text).collect();
        assert_eq!(texts, ["M3 S0", "", "S0 ", "G1 X1 F100"]);
        assert!(job.text().ends_with("G1 X1 F100\n"));
    }

    #[test]
    fn images_are_not_planned_yet() {
        let mut d = Document::new();
        let l = d.layer_for_color(Color::BLACK, 1, 1);
        d.add_object(
            l,
            ObjectKind::Image(Box::new(ImageObject {
                data: ImageData::Rgba {
                    width: 0,
                    height: 0,
                    pixels: Bytes::default(),
                },
                position: Point::ZERO,
                size_mm: [1.0, 1.0],
                settings: RasterSettings::default(),
                edits: Vec::new(),
            })),
        )
        .unwrap();
        // images are planned (an empty one engraves nothing visible)
        assert!(plan(&d, &PlanOptions::default()).is_ok());
        let mut c = d.clone();
        if let ObjectKind::Image(img) = &mut c.objects[0].kind {
            img.settings.tool = freekerf_document::RasterTool::Centerline;
        }
        assert!(matches!(
            plan(&c, &PlanOptions::default()),
            Err(PlanError::Unsupported(_))
        ));
        assert!(PlanError::Image("x".into()).to_string().contains("image"));
        let e: PlanError = freekerf_geometry::clipper::ClipperError::Range.into();
        assert!(e.to_string().contains("range"));
    }
}
