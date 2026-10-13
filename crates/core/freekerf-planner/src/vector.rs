//! Colour-layer jobs from vector shapes, written the way LaserGRBL writes them:
//! `GCodeFromSVG.convertFromXml` for SVG sources and `VectorGCode` for DXF ones.
//!
//! Per layer in execution order and per pass: a comment, the layer's feed and power,
//! the filling (if any), then the outlines (if the mode has them). The two sources
//! differ in details the G-code shows: SVG pen up/down lines carry a comment (they
//! keep a trailing space once loaded: `S0 `), the SVG writer starts with the pen
//! "down" (its first pen up is written), SVG arcs are always `G2`/`G3` while DXF
//! arcs too short or too flat become lines, SVG fillings approximate arcs with
//! ~0.2 mm chords and DXF ones within 0.01 mm, and DXF passes end with a pen up.

use std::f64::consts::PI;

use freekerf_document::{Document, Layer, ObjectKind, SourceFormat, UnsupportedReason};
use freekerf_geometry::{Path, Point, Segment, arc_sweep};

use crate::writer::Writer;
use crate::{PlanError, PlanOptions, fill};

/// How LaserGRBL wrote vector jobs of a source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorStyle {
    /// `GCodeFromSVG`.
    Svg,
    /// `VectorGCode` (DXF).
    Dxf,
}

impl VectorStyle {
    /// The style of a document: SVG when any object comes from an SVG file.
    #[must_use]
    pub fn of(doc: &Document) -> Self {
        let svg = doc
            .objects
            .iter()
            .any(|o| o.source.as_ref().is_some_and(|s| s.format == SourceFormat::Svg));
        if svg { Self::Svg } else { Self::Dxf }
    }
}

/// Arcs shorter (mm) are written as lines (DXF).
const MIN_ARC_LENGTH: f64 = 0.05;
/// Arcs closer to their chord (mm) are written as lines (DXF).
const MIN_ARC_SAGITTA: f64 = 0.001;
/// Arc approximation for DXF fillings, mm.
const DXF_FILL_TOLERANCE: f64 = 0.01;

/// `VectorGCode.IsTrueArc`: almost straight arcs have huge radii that the controller
/// may reject; a line is as precise.
fn is_true_arc(start: Point, end: Point, center: Point, ccw: bool) -> bool {
    let sweep = arc_sweep(start, end, center, ccw).abs();
    let r = end.distance(center);
    let sagitta = if sweep >= PI {
        r
    } else {
        r * (1.0 - (sweep / 2.0).cos())
    };
    sweep * r >= MIN_ARC_LENGTH && sagitta >= MIN_ARC_SAGITTA
}

/// `captureArcCCW`: about 0.2 mm chords (at least 8) of a counterclockwise arc.
#[allow(clippy::cast_possible_truncation)]
fn capture_arc_ccw(shape: &mut Vec<Point>, start: Point, end: Point, ij: Point) {
    let (cx, cy) = (start.x + ij.x, start.y + ij.y);
    let r = (ij.x * ij.x + ij.y * ij.y).sqrt();
    let a1 = (start.y - cy).atan2(start.x - cx);
    let a2 = (end.y - cy).atan2(end.x - cx);
    let mut sweep = a2 - a1;
    if sweep <= 1e-9 {
        sweep += 2.0 * PI; // G3 is counterclockwise, same start and end is a full circle
    }
    let segments = ((sweep * r / 0.2).ceil() as i32).max(8);
    for i in 1..segments {
        let a = a1 + sweep * f64::from(i) / f64::from(segments);
        shape.push(Point::new(cx + r * a.cos(), cy + r * a.sin()));
    }
    shape.push(end);
}

/// The shapes a filling is computed from (`CaptureShapes` / `ToPolyline`).
fn capture(paths: &[&Path], style: VectorStyle) -> Vec<Vec<Point>> {
    paths
        .iter()
        .map(|p| match style {
            VectorStyle::Dxf => p.to_polyline(DXF_FILL_TOLERANCE),
            VectorStyle::Svg => {
                let mut shape = vec![p.start];
                let mut current = p.start;
                for s in &p.segments {
                    match *s {
                        Segment::Arc { end, center, .. } => {
                            capture_arc_ccw(&mut shape, current, end, center - current);
                        }
                        Segment::Line { end } => shape.push(end),
                        Segment::Cubic { c1, c2, end } => {
                            let pts = freekerf_geometry::bezier::flatten(
                                &[current, c1, c2, end],
                                0.2,
                                freekerf_geometry::bezier::MAX_SUBDIVISIONS,
                            );
                            shape.extend(pts.into_iter().skip(1));
                        }
                    }
                    current = s.end();
                }
                shape
            }
        })
        .collect()
}

struct Emitter {
    w: Writer,
    style: VectorStyle,
    pen_down: bool,
}

impl Emitter {
    fn cmt(&self, c: &'static str) -> &'static str {
        if self.style == VectorStyle::Svg { c } else { "" }
    }

    fn pen_up(&mut self, c: &'static str) {
        if self.pen_down {
            let c = self.cmt(c);
            self.w.pen_up(c);
        }
        self.pen_down = false;
    }

    fn pen_down(&mut self, c: &'static str) {
        if !self.pen_down {
            let c = self.cmt(c);
            self.w.pen_down(c);
        }
        self.pen_down = true;
    }

    fn polylines(&mut self, lines: &[Vec<Point>]) {
        for l in lines {
            self.pen_up("fill");
            let c = self.cmt("fill");
            self.w.move_to_rapid(l[0], c);
            for p in &l[1..] {
                self.pen_down("fill");
                let c = self.cmt("fill");
                self.w.move_to(*p, c);
            }
        }
        if self.style == VectorStyle::Svg {
            self.pen_up("fill");
        }
    }

    fn path(&mut self, p: &Path) {
        if !p.from_current {
            self.pen_up("path");
            let c = self.cmt("path");
            self.w.move_to_rapid(p.start, c);
        }
        let mut current = p.start;
        for s in &p.segments {
            self.pen_down("path");
            let c = self.cmt("path");
            match *s {
                Segment::Line { end } => self.w.move_to(end, c),
                Segment::Arc { end, center, ccw } => {
                    if self.style == VectorStyle::Svg || is_true_arc(current, end, center, ccw) {
                        self.w
                            .arc(if ccw { 3 } else { 2 }, end, center - current, c, false);
                    } else {
                        self.w.move_to(end, c);
                    }
                }
                Segment::Cubic { c1, c2, end } => {
                    let pts = freekerf_geometry::bezier::flatten(
                        &[current, c1, c2, end],
                        0.01,
                        freekerf_geometry::bezier::MAX_SUBDIVISIONS,
                    );
                    for q in pts.into_iter().skip(1) {
                        self.w.move_to(q, c);
                    }
                }
            }
            current = s.end();
        }
    }

    fn unsupported(&mut self, element: &str, reason: UnsupportedReason) {
        // the first basic shape of a pass lifts the pen ("1st shape"); the pen is down
        // only before anything was written
        self.pen_up("1st shape");
        match reason {
            UnsupportedReason::NotConverted => {
                self.w.raw("( +++++++++++++++++++++++++++++++++ )");
                self.w.raw(format!("( ++++++ {element} is not supported ++++ )"));
                if element == "text" {
                    self.w.raw("( ++ Convert Object to Path first + )");
                }
                self.w.raw("( +++++++++++++++++++++++++++++++++ )");
            }
            UnsupportedReason::MissingComma => self.w.raw("( polygon coordinates - missing '.')"),
            UnsupportedReason::MissingArgument => self.w.raw("( Missing argument )"),
        }
    }
}

/// The hatch lines of a filling layer (`fill`, `fill_and_line`) as its job engraves
/// them, in the writing style of `doc` (the Design mode draws them); none for the
/// other modes.
///
/// # Errors
/// The filling failed (coordinates out of range).
pub fn layer_filling(doc: &Document, layer: &Layer) -> Result<Vec<Vec<Point>>, PlanError> {
    filling(doc, layer, VectorStyle::of(doc))
}

fn filling(doc: &Document, layer: &Layer, style: VectorStyle) -> Result<Vec<Vec<Point>>, PlanError> {
    if !layer.mode.has_fill() {
        return Ok(Vec::new());
    }
    let paths: Vec<&Path> = doc
        .objects_on(layer.id)
        .filter_map(|o| match &o.kind {
            ObjectKind::Shape { paths } => Some(paths.iter()),
            _ => None,
        })
        .flatten()
        .collect();
    fill::build(
        &capture(&paths, style),
        layer.fill.pattern,
        layer.fill.lines_per_mm,
    )
}

/// The G-code lines of the vector objects of `doc` (colour layers).
///
/// # Errors
/// A filling failed (coordinates out of range).
pub fn plan_vectors(doc: &Document, options: &PlanOptions) -> Result<Vec<String>, PlanError> {
    let style = options.vector_style.unwrap_or_else(|| VectorStyle::of(doc));
    let mut e = Emitter {
        w: Writer::new(&options.machine),
        style,
        pen_down: style == VectorStyle::Svg,
    };
    // the laser on command of each layer (`M3`/`M4`), the first one from the start
    let order = doc.execution_order();
    let laser_on = |l: &Layer| {
        l.laser_mode
            .command()
            .unwrap_or(&options.machine.laser_on)
            .to_owned()
    };
    if let Some(first) = order.first() {
        e.w.set_laser_on(&laser_on(first), false);
    }
    e.w.initial();
    for layer in order {
        let objects: Vec<_> = doc.objects_on(layer.id).collect();
        if !objects.is_empty() {
            e.w.set_laser_on(&laser_on(layer), true);
        }
        let filling = if layer.mode.has_fill() {
            Some(filling(doc, layer, style)?)
        } else {
            None
        };
        for pass in 1..=layer.passes {
            if options.layer_preamble {
                e.w.raw(format!(
                    "(Layer {} {} pass {pass}/{})",
                    layer.color,
                    layer.mode.lasergrbl_name(),
                    layer.passes
                ));
                e.w.set_layer_params(layer.speed_mm_min, layer.power);
            }
            if let Some(lines) = &filling {
                e.polylines(lines);
            }
            if layer.mode.has_outline() {
                for o in &objects {
                    match &o.kind {
                        ObjectKind::Shape { paths } => {
                            for p in paths {
                                e.path(p);
                            }
                            if style == VectorStyle::Svg {
                                e.pen_up("end");
                            }
                        }
                        ObjectKind::Unsupported { element, reason } if style == VectorStyle::Svg => {
                            e.unsupported(element, *reason);
                        }
                        _ => {}
                    }
                }
            }
            if style == VectorStyle::Dxf {
                e.pen_up("");
            }
        }
    }
    e.w.final_command();
    Ok(e.w.into_lines())
}

#[cfg(test)]
mod tests {
    use freekerf_document::{Color, LayerMode, Source};
    use freekerf_geometry::hatch::FillPattern;

    use super::*;

    fn square(x: f64) -> Path {
        Path::polyline(
            &[
                Point::new(x, 0.0),
                Point::new(x + 10.0, 0.0),
                Point::new(x + 10.0, 10.0),
                Point::new(x, 10.0),
            ],
            true,
        )
    }

    fn doc(format: SourceFormat, mode: LayerMode) -> Document {
        let mut d = Document::new();
        let l = d.layer_for_color(Color::BLACK, 1000, 500);
        d.layer_mut(l).unwrap().mode = mode;
        d.layer_mut(l).unwrap().fill.pattern = FillPattern::Horizontal;
        d.layer_mut(l).unwrap().fill.lines_per_mm = 1.0;
        let mut circle = Path::new(Point::new(30.0, 5.0));
        circle
            .segments
            .push(Segment::arc(Point::new(30.0, 5.0), Point::new(25.0, 5.0), true));
        d.add_object_with(
            l,
            ObjectKind::Shape {
                paths: vec![square(0.0), circle],
            },
            String::new(),
            Some(Source {
                format,
                file: String::new(),
            }),
        )
        .unwrap();
        d
    }

    #[test]
    fn svg_style_lines() {
        let d = doc(SourceFormat::Svg, LayerMode::Line);
        let lines = plan_vectors(&d, &PlanOptions::default()).unwrap();
        assert_eq!(lines[0], "M3 S0");
        assert_eq!(lines[1], "(Layer #000000 Line pass 1/1)");
        assert_eq!(lines[2], "S0 (path)");
        assert_eq!(lines[3], "G0X0Y0(path)");
        assert_eq!(lines[4], "S500 (path)");
        assert_eq!(lines[5], "G1X10F1000(path)");
        assert!(lines.iter().any(|l| l.starts_with("G3X30Y5I-5J0")));
        assert_eq!(lines.last().unwrap(), "M5 S0");
    }

    #[test]
    fn laser_mode_per_layer() {
        use freekerf_document::LaserMode;
        // a dynamic-power line layer, then a constant-power cut layer
        let mut d = doc(SourceFormat::Svg, LayerMode::Line);
        d.layers[0].laser_mode = LaserMode::Dynamic;
        let cut = d.layer_for_color(Color::rgb(255, 0, 0), 300, 1000);
        d.layer_mut(cut).unwrap().mode = LayerMode::Cut;
        d.layer_mut(cut).unwrap().laser_mode = LaserMode::Constant;
        d.add_object(
            cut,
            ObjectKind::Shape {
                paths: vec![square(50.0)],
            },
        )
        .unwrap();
        let lines = plan_vectors(&d, &PlanOptions::default()).unwrap();
        // the first layer's command from the start, the next one when its layer begins
        assert_eq!(lines[0], "M4 S0");
        let cut_at = lines
            .iter()
            .position(|l| l.starts_with("(Layer #FF0000"))
            .unwrap();
        assert_eq!(lines[cut_at - 1], "M3");
        assert_eq!(lines.iter().filter(|l| *l == "M3" || *l == "M4").count(), 1);
        // the machine's command when the layers leave it (unchanged output)
        let mut options = PlanOptions::default();
        options.machine.laser_on = "M4".into();
        d.layers
            .iter_mut()
            .for_each(|l| l.laser_mode = LaserMode::Machine);
        let lines = plan_vectors(&d, &options).unwrap();
        assert_eq!(lines[0], "M4 S0");
        assert!(!lines.iter().any(|l| l == "M3" || l == "M4"));
        // without PWM, pen down writes the layer's command
        let mut d = doc(SourceFormat::Svg, LayerMode::Line);
        d.layers[0].laser_mode = LaserMode::Dynamic;
        let mut options = PlanOptions::default();
        options.machine.support_pwm = false;
        let lines = plan_vectors(&d, &options).unwrap();
        assert!(lines.iter().any(|l| l.starts_with("M4")), "{lines:?}");
        assert!(!lines.iter().any(|l| l.starts_with("M3")), "{lines:?}");
    }

    #[test]
    fn dxf_style_fill_and_line() {
        let d = doc(SourceFormat::Dxf, LayerMode::FillAndLine);
        let lines = plan_vectors(&d, &PlanOptions::default()).unwrap();
        // first hatch line (nearest to the origin; the clipper may reverse lines): no
        // comments, and the pen starts up
        assert_eq!(lines[2], "G0X0Y2");
        assert_eq!(lines[3], "S500");
        assert!(lines.iter().any(|l| l.starts_with("G3X30Y5I-5J0")));
        assert!(lines.iter().filter(|l| l.as_str() == "S0").count() > 5);
    }

    #[test]
    fn unsupported_elements_write_comments_in_svg_style() {
        let mut d = doc(SourceFormat::Svg, LayerMode::Line);
        let l = d.layers[0].id;
        for (element, reason) in [
            ("text", UnsupportedReason::NotConverted),
            ("image", UnsupportedReason::NotConverted),
            ("polygon", UnsupportedReason::MissingComma),
            ("path", UnsupportedReason::MissingArgument),
        ] {
            d.add_object(
                l,
                ObjectKind::Unsupported {
                    element: element.into(),
                    reason,
                },
            )
            .unwrap();
        }
        let lines = plan_vectors(&d, &PlanOptions::default()).unwrap();
        assert_eq!(lines.iter().filter(|s| s.contains("not supported")).count(), 2);
        assert_eq!(lines.iter().filter(|s| s.contains("Convert Object")).count(), 1);
        assert_eq!(lines.iter().filter(|s| s.contains("missing")).count(), 1);
        assert_eq!(lines.iter().filter(|s| s.contains("Missing argument")).count(), 1);
    }

    #[test]
    fn short_or_flat_dxf_arcs_become_lines() {
        assert!(!is_true_arc(
            Point::new(1.0, 0.0),
            Point::new(1.0, 0.01),
            Point::ZERO,
            true
        ));
        assert!(is_true_arc(
            Point::new(1.0, 0.0),
            Point::new(0.0, 1.0),
            Point::ZERO,
            true
        ));
        let mut shape = vec![Point::new(1.0, 0.0)];
        capture_arc_ccw(
            &mut shape,
            Point::new(1.0, 0.0),
            Point::new(-1.0, 0.0),
            Point::new(-1.0, 0.0),
        );
        assert_eq!(shape.len(), 17); // ceil(pi / 0.2) = 16 chords
    }
}
