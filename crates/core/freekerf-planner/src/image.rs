//! Images to G-code, as LaserGRBL's raster dialog generates them
//! (`ImageProcessor.DoTrueWork`): the edits, the processed bitmap
//! (`freekerf_raster::processor`) and the tool's job — line by line
//! (`LoadImageL2L`), vectorize (`LoadImagePotrace`) or centerline
//! (`LoadImageCenterline`, through a [`Tracer`]).

use freekerf_document::{
    Color, Document, ImageData, ImageEdit, ImageObject, Layer, LayerId, RasterSettings, RasterTool, Source,
    SourceFormat,
};
use freekerf_gcode::Command;
use freekerf_import::svg;
use freekerf_raster::centerline::{
    CenterlineOptions, INPUT_PLACEHOLDER, Tracer, autotrace_args, max_size_mm,
};
use freekerf_raster::image::Image;
use freekerf_raster::l2l::{Direction, L2lConf, L2lOptions, load_image_l2l};
use freekerf_raster::potrace::TraceOptions;
use freekerf_raster::processor::{self, ProcessError};
use freekerf_raster::vectorize::{PotraceOptions, load_image_potrace};

use crate::{Firmware, Job, MachineOptions, PlanError, PlanOptions, VectorStyle};

/// The resolution of a file without one, dots per inch (GDI+'s default).
pub const DEFAULT_DPI: u32 = 96;

/// What an image job needs besides the image.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageJob<'a> {
    /// The raster dialog values.
    pub settings: &'a RasterSettings,
    /// Edits applied first.
    pub edits: &'a [ImageEdit],
    /// Size on the bed, mm.
    pub size_mm: [f64; 2],
    /// Lower-left corner on the bed, mm (single precision, as LaserGRBL keeps it).
    pub offset: (f32, f32),
    /// The file's resolution (pass-through engraves at it).
    pub file_dpi: u32,
    /// `Vector.UseSmartBezier` (centerline's SVG conversion).
    pub smart_bezier: bool,
}

/// Decodes an image of the document.
///
/// # Errors
/// Not a supported image file, or pixel data of the wrong size.
pub fn decode(data: &ImageData) -> Result<Image, PlanError> {
    let r = match data {
        ImageData::Encoded { bytes, .. } => Image::decode(&bytes.0),
        ImageData::Rgba {
            width,
            height,
            pixels,
        } => Image::from_rgba(*width, *height, &pixels.0),
    };
    r.map_err(|e| PlanError::Image(e.to_string()))
}

#[allow(clippy::needless_pass_by_value)] // used as `map_err(image_error)`
fn image_error(e: ProcessError) -> PlanError {
    PlanError::Image(e.to_string())
}

/// The job of an SVG printed by the centerline tracer (`LoadImageCenterline`'s
/// conversion half, also the `svg_text` conformance kind): every element on one
/// layer, scaled so its larger side is `max_size` mm, moved by `offset` mm, with the
/// machine options' default speed, power and laser commands.
///
/// # Errors
/// The SVG cannot be read.
pub fn replay_svg(
    svg_bytes: &[u8],
    max_size: f32,
    offset: (f32, f32),
    smart_bezier: bool,
    machine: &MachineOptions,
) -> Result<Job, PlanError> {
    let opts = svg::SvgOptions {
        scale_to_mm: Some(max_size),
        user_offset: offset,
        legacy_bezier: !smart_bezier,
        // vectorized images have no text
        fonts: None,
    };
    let import = svg::import(svg_bytes, &opts).map_err(|e| PlanError::Image(e.to_string()))?;
    let doc = import.into_single_layer(Some(&Source {
        format: SourceFormat::Svg,
        file: String::new(),
    }));
    crate::plan(
        &doc,
        &PlanOptions {
            machine: machine.clone(),
            vector_style: Some(VectorStyle::Svg),
            layer_preamble: false,
        },
    )
}

/// What a centerline conversion did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Centerline {
    /// Arguments given to the tracer, the input as [`INPUT_PLACEHOLDER`].
    pub args: Vec<String>,
    /// The PNG given to the tracer.
    pub png: Vec<u8>,
    /// Program lines, as LaserGRBL loads them (trimmed, comment-only lines dropped).
    pub lines: Vec<String>,
}

/// `GrblFile.LoadImageCenterline`: `img` (already processed) to the tracer, its
/// SVG to G-code scaled to a tenth of the image's pixel size.
///
/// # Errors
/// The tracer fails or prints nothing (LaserGRBL then fails on the empty output as
/// "Root element is missing"; the cause is reported instead, DIV-048), or its SVG
/// cannot be converted.
pub fn centerline(
    img: &Image,
    options: &CenterlineOptions,
    tracer: &dyn Tracer,
    offset: (f32, f32),
    smart_bezier: bool,
    machine: &MachineOptions,
) -> Result<Centerline, PlanError> {
    let png = img.encode_png().map_err(|e| PlanError::Image(e.to_string()))?;
    let args = autotrace_args(options, INPUT_PLACEHOLDER);
    let svg_text = tracer
        .trace(&png, &args)
        .map_err(|e| PlanError::Image(format!("autotrace: {e}")))?;
    if svg_text.trim().is_empty() {
        return Err(PlanError::Image("autotrace printed nothing".into()));
    }
    let job = replay_svg(
        svg_text.as_bytes(),
        max_size_mm(img.width(), img.height()),
        offset,
        smart_bezier,
        machine,
    )?;
    // LaserGRBL trims every line and keeps the commands that are not empty once
    // their comments are removed (`S0 (M)` stays as `S0 `, as `GrblCommand` reads it)
    let lines = job
        .lines
        .iter()
        .map(|l| l.trim().to_owned())
        .filter(|l| !l.is_empty() && !Command::new(l).is_empty())
        .collect();
    Ok(Centerline { args, png, lines })
}

/// `ImageProcessor.DoTrueWork`: the G-code lines of an image (decoded file pixels,
/// before edits). Centerline needs a `tracer`.
///
/// # Errors
/// The resolution comes out as zero ("target size too big"), centerline without a
/// tracer, or a failing tracer.
pub fn plan_image(
    original: &Image,
    job: &ImageJob<'_>,
    machine: &MachineOptions,
    tracer: Option<&dyn Tracer>,
) -> Result<Vec<String>, PlanError> {
    let s = job.settings;
    let original = processor::normalize(original);
    let gray = processor::is_gray(&original);
    let edited = processor::apply_edits(&original, job.edits);
    let prepared = processor::prepare(&edited, gray, s, job.size_mm, job.file_dpi).map_err(image_error)?;
    #[allow(clippy::cast_possible_wrap)]
    let conf = L2lConf {
        res: prepared.res,
        fres: prepared.fres,
        offset_x: job.offset.0,
        offset_y: job.offset.1,
        mark_speed: s.mark_speed_mm_min as i32,
        border_speed: s.border_speed_mm_min as i32,
        min_power: s.min_power as i32,
        max_power: s.max_power as i32,
        laser_on: s.laser_on.clone(),
        laser_off: s.laser_off.clone(),
        direction: if s.tool == RasterTool::Passthrough {
            Direction::Horizontal
        } else {
            processor::direction(s.direction)
        },
        pwm: machine.support_pwm,
        vector_filling: false,
        smoothie: machine.firmware == Firmware::Smoothie,
    };
    let opts = L2lOptions {
        disable_g0: machine.disable_g0,
        unidirectional: machine.unidirectional,
        max_pwm: machine.max_pwm,
    };
    match s.tool {
        RasterTool::LineToLine | RasterTool::Dithering | RasterTool::Passthrough => {
            Ok(load_image_l2l(&prepared.bitmap, &conf, &opts))
        }
        RasterTool::Vectorize => {
            let v = &s.vectorize;
            #[allow(clippy::cast_possible_wrap)]
            let trace = TraceOptions {
                turdsize: v.spot_removal.map_or(2, |t| t as i32),
                alphamax: v.smoothing.unwrap_or(0.0),
                curve_optimizing: v.optimize.is_some(),
                opttolerance: v.optimize.unwrap_or(0.2),
            };
            let p = PotraceOptions {
                trace,
                optimize_fast: v.optimize_fast,
                filling: processor::filling(v.fill),
            };
            Ok(load_image_potrace(
                &prepared.bitmap,
                &p,
                &conf,
                &opts,
                machine.support_pwm,
            ))
        }
        RasterTool::Centerline => {
            let tracer = tracer
                .ok_or_else(|| PlanError::Unsupported("centerline needs the autotrace program".into()))?;
            #[allow(clippy::cast_possible_wrap)]
            let options = CenterlineOptions {
                corner_threshold: s.centerline.corner_threshold.map(|v| v as i32),
                line_threshold: s.centerline.line_threshold.map(|v| v as i32),
            };
            // LaserGRBL's centerline takes speed, power and laser commands from the
            // dialog settings (DIV-049): the image's settings here
            let replay = MachineOptions {
                laser_on: s.laser_on.clone(),
                laser_off: s.laser_off.clone(),
                default_border_speed: s.border_speed_mm_min,
                default_power_max: s.max_power,
                ..machine.clone()
            };
            Ok(centerline(
                &prepared.bitmap,
                &options,
                tracer,
                job.offset,
                job.smart_bezier,
                &replay,
            )?
            .lines)
        }
    }
}

/// The job of an image object of a document.
///
/// # Errors
/// As [`plan_image`], or pixels that cannot be decoded.
#[allow(clippy::cast_possible_truncation)]
pub fn plan_image_object(
    obj: &ImageObject,
    machine: &MachineOptions,
    tracer: Option<&dyn Tracer>,
) -> Result<Vec<String>, PlanError> {
    let img = decode(&obj.data)?;
    plan_image(
        &img,
        &ImageJob {
            settings: &obj.settings,
            edits: &obj.edits,
            size_mm: obj.size_mm,
            offset: (obj.position.x as f32, obj.position.y as f32),
            file_dpi: DEFAULT_DPI,
            smart_bezier: true,
        },
        machine,
        tracer,
    )
}

/// The bitmap the raster dialog's settings make of an image object (what
/// line-by-line engraving burns: grey levels, or black and white when dithered or
/// thresholded), for previews.
///
/// # Errors
/// The image cannot be decoded, or the resolution comes out as zero.
pub fn preview_bitmap(obj: &ImageObject, settings: &RasterSettings) -> Result<Image, PlanError> {
    let original = processor::normalize(&decode(&obj.data)?);
    let gray = processor::is_gray(&original);
    let edited = processor::apply_edits(&original, &obj.edits);
    let prepared =
        processor::prepare(&edited, gray, settings, obj.size_mm, DEFAULT_DPI).map_err(image_error)?;
    Ok(prepared.bitmap)
}

/// A document holding one image on its own layer (for tests and scripts).
#[must_use]
pub fn single_image_document(obj: ImageObject) -> Document {
    let mut doc = Document::new();
    let layer = doc.add_layer(Layer::new(LayerId(0), Color::BLACK, 0, 0));
    let _ = doc.add_object_with(
        layer,
        freekerf_document::ObjectKind::Image(Box::new(obj)),
        String::new(),
        None,
    );
    doc
}

#[cfg(test)]
mod tests {
    use freekerf_document::{Bytes, VectorFill};
    use freekerf_geometry::Point;
    use freekerf_raster::image::{BLACK, WHITE};

    use super::*;

    struct Fixed(Result<String, String>);

    impl Tracer for Fixed {
        fn trace(&self, png: &[u8], args: &[String]) -> Result<String, String> {
            assert!(png.starts_with(b"\x89PNG"));
            assert_eq!(args.last().map(String::as_str), Some(INPUT_PLACEHOLDER));
            self.0.clone()
        }
    }

    const SVG: &str =
        r#"<svg width="120" height="80"><path style="stroke:#000000; fill:none;" d="M10 70L50 70"/></svg>"#;

    fn ring() -> Image {
        let px = (0..64)
            .map(|i| {
                let (x, y) = (i % 8, i / 8);
                if (2..6).contains(&x) && (2..6).contains(&y) {
                    BLACK
                } else {
                    WHITE
                }
            })
            .collect();
        Image::from_pixels(8, 8, px).unwrap()
    }

    fn job(settings: &RasterSettings) -> ImageJob<'_> {
        ImageJob {
            settings,
            edits: &[],
            size_mm: [0.8, 0.8],
            offset: (1.0, 2.0),
            file_dpi: DEFAULT_DPI,
            smart_bezier: true,
        }
    }

    #[test]
    fn every_tool() {
        let m = MachineOptions::default();
        for tool in [
            RasterTool::LineToLine,
            RasterTool::Dithering,
            RasterTool::Passthrough,
            RasterTool::Vectorize,
        ] {
            let mut s = RasterSettings {
                tool,
                lines_per_mm: 10.0,
                ..RasterSettings::default()
            };
            s.vectorize.fill = Some(VectorFill::Raster(freekerf_document::LineDirection::Horizontal));
            let lines = plan_image(&ring(), &job(&s), &m, None).unwrap();
            assert!(lines.iter().any(|l| l.contains("G1")), "{tool:?}: {lines:?}");
        }
        let s = RasterSettings {
            tool: RasterTool::Centerline,
            ..RasterSettings::default()
        };
        assert!(matches!(
            plan_image(&ring(), &job(&s), &m, None),
            Err(PlanError::Unsupported(_))
        ));
        let lines = plan_image(&ring(), &job(&s), &m, Some(&Fixed(Ok(SVG.into())))).unwrap();
        assert!(lines.iter().all(|l| !Command::new(l).is_empty()));
        assert!(plan_image(&ring(), &job(&s), &m, Some(&Fixed(Err("boom".into())))).is_err());
        assert!(plan_image(&ring(), &job(&s), &m, Some(&Fixed(Ok(" ".into())))).is_err());
        assert!(plan_image(&ring(), &job(&s), &m, Some(&Fixed(Ok("<svg".into())))).is_err());
        let zero = RasterSettings {
            lines_per_mm: 0.0,
            ..RasterSettings::default()
        };
        assert!(matches!(
            plan_image(&ring(), &job(&zero), &m, None),
            Err(PlanError::Image(_))
        ));
    }

    #[test]
    fn image_objects() {
        let img = ring();
        let obj = ImageObject {
            data: ImageData::Encoded {
                media_type: "image/png".into(),
                bytes: Bytes(img.encode_png().unwrap()),
            },
            position: Point::new(0.0, 0.0),
            size_mm: [0.8, 0.8],
            settings: RasterSettings {
                lines_per_mm: 10.0,
                ..RasterSettings::default()
            },
            edits: vec![ImageEdit::Invert],
        };
        assert!(
            !plan_image_object(&obj, &MachineOptions::default(), None)
                .unwrap()
                .is_empty()
        );
        let raw = ImageObject {
            data: ImageData::Rgba {
                width: 8,
                height: 8,
                pixels: Bytes(img.to_rgba()),
            },
            ..obj.clone()
        };
        assert!(plan_image_object(&raw, &MachineOptions::default(), None).is_ok());
        let bad = ImageObject {
            data: ImageData::Rgba {
                width: 9,
                height: 8,
                pixels: Bytes(img.to_rgba()),
            },
            ..obj.clone()
        };
        assert!(plan_image_object(&bad, &MachineOptions::default(), None).is_err());
        let doc = single_image_document(obj);
        assert_eq!(doc.objects.len(), 1);
    }
}
