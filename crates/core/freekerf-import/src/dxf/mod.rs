//! ASCII DXF import as LaserGRBL reads it (`SvgConverter/DxfReader.cs`): lines and
//! true arcs in millimetres, one colour per DXF colour (true colour, ACI, ByLayer,
//! ByBlock), blocks (`INSERT`, `MINSERT`, nested up to 32 levels as in LaserGRBL,
//! at most a million drawn entities), `LWPOLYLINE` and
//! `POLYLINE` with bulges, `CIRCLE`, `ARC`, `ELLIPSE` (lines), `SPLINE` (sampled,
//! then fitted with arcs within a tolerance). The drawing is moved so its bounds
//! start at the origin.

mod aci;

use std::collections::HashMap;
use std::f64::consts::PI;

use freekerf_document::{Color, ObjectKind};
use freekerf_geometry::{Path, Point, Rect, Segment, arcfit};

use crate::netparse::{parse_double, parse_long};
use crate::svg::decode_text;
use crate::{ColorCount, Import, ImportError, ImportedObject};

/// Protection against deeply nested blocks.
const MAX_BLOCK_DEPTH: u32 = 32;
/// The most entities a drawing may expand to through its blocks (`INSERT`,
/// `MINSERT` grids): beyond it the file is refused (DIV-109).
const MAX_DRAWN: usize = 1 << 20;
/// The most path segments an import may produce (a few hundred MB at most).
const MAX_SEGMENTS: usize = 1 << 21;
/// Max distance (mm) between an arc and its approximation.
const ARC_TOLERANCE: f64 = 0.02;
/// Length (mm) of the segments that sample splines.
const SPLINE_STEP: f64 = 0.1;

/// Options of the DXF import.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DxfOptions {
    /// Splines are fitted with lines and arcs within this distance (mm); 0 keeps
    /// them as short lines (`Vector.SplineArcTolerance`).
    pub spline_tolerance: f64,
}

impl Default for DxfOptions {
    fn default() -> Self {
        Self {
            spline_tolerance: arcfit::DEFAULT_TOLERANCE,
        }
    }
}

/// An entity (or table record): its code/value pairs.
#[derive(Debug, Clone, Default)]
struct Entity {
    kind: String,
    pairs: Vec<(i32, String)>,
    vertices: Vec<Entity>,
    sequence_ended: bool,
}

fn bad_number(v: &str) -> ImportError {
    ImportError::Invalid(format!("`{v}` is not a number"))
}

impl Entity {
    fn has(&self, code: i32) -> bool {
        self.pairs.iter().any(|(c, _)| *c == code)
    }

    fn string(&self, code: i32, def: &str) -> String {
        self.pairs
            .iter()
            .find(|(c, _)| *c == code)
            .map_or_else(|| def.to_owned(), |(_, v)| v.clone())
    }

    fn double(&self, code: i32, def: f64) -> Result<f64, ImportError> {
        match self.pairs.iter().find(|(c, _)| *c == code) {
            Some((_, v)) => parse_double(v).ok_or_else(|| bad_number(v)),
            None => Ok(def),
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    fn int(&self, code: i32, def: i32) -> Result<i32, ImportError> {
        match self.pairs.iter().find(|(c, _)| *c == code) {
            // (int)long.Parse: the low 32 bits
            Some((_, v)) => parse_long(v).map(|n| n as i32).ok_or_else(|| bad_number(v)),
            None => Ok(def),
        }
    }

    fn all(&self, code: i32) -> Result<Vec<f64>, ImportError> {
        self.pairs
            .iter()
            .filter(|(c, _)| *c == code)
            .map(|(_, v)| parse_double(v).ok_or_else(|| bad_number(v)))
            .collect()
    }

    /// The extrusion direction points down: the object coordinate system is mirrored on X.
    fn mirrored_ocs(&self) -> Result<bool, ImportError> {
        Ok(self.double(230, 1.0)? < 0.0)
    }
}

#[derive(Debug, Clone, Default)]
struct Block {
    name: String,
    base: (f64, f64),
    entities: Vec<Entity>,
}

#[derive(Debug, Clone)]
struct DxfLayer {
    color: Color,
    hidden: bool,
}

#[derive(Debug, Default)]
struct DxfDocument {
    unit_to_mm: f64,
    units: i64,
    layers: HashMap<String, DxfLayer>,
    blocks: HashMap<String, Block>,
    entities: Vec<Entity>,
}

fn key(name: &str) -> String {
    name.to_uppercase()
}

fn units_to_mm(units: i64) -> f64 {
    match units {
        1 => 25.4,
        2 => 304.8,
        5 => 10.0,
        6 => 1000.0,
        8 => 0.000_025_4,
        9 => 0.0254,
        10 => 914.4,
        13 => 0.001,
        14 => 100.0,
        _ => 1.0, // 4 = millimetres; unitless or unknown: assume mm
    }
}

#[allow(clippy::cast_sign_loss)]
fn true_color(rgb: i32) -> Color {
    let v = (rgb & 0x00FF_FFFF) as u32;
    Color::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

fn aci_color(index: i32) -> Color {
    // 7 is white on a black background and black on a white one: for the laser it is black
    match usize::try_from(index) {
        Ok(i) if i > 0 && i != 7 && i < aci::PALETTE.len() => {
            true_color(i32::try_from(aci::PALETTE[i]).unwrap_or(0))
        }
        _ => Color::BLACK,
    }
}

/// `File.ReadAllLines`: lines split on `\r\n`, `\n` or `\r`, no last empty line.
fn lines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        match rest.find(['\r', '\n']) {
            Some(i) => {
                out.push(&rest[..i]);
                let skip = if rest[i..].starts_with("\r\n") { 2 } else { 1 };
                rest = &rest[i + skip..];
            }
            None => {
                out.push(rest);
                rest = "";
            }
        }
    }
    out
}

impl DxfDocument {
    fn load(lines: &[&str]) -> Result<Self, ImportError> {
        let mut doc = Self {
            unit_to_mm: 1.0,
            ..Self::default()
        };
        // split the file in records, each one starts with a code 0 pair
        let mut records: Vec<Entity> = Vec::new();
        let mut i = 0;
        while i + 1 < lines.len() {
            let code_text = lines[i].trim();
            if code_text.is_empty() && i + 2 >= lines.len() {
                break; // empty line at the end of the file
            }
            let code = parse_long(code_text)
                .and_then(|c| i32::try_from(c).ok())
                .ok_or_else(|| {
                    ImportError::Invalid(format!("line {}: `{code_text}` is not a group code", i + 1))
                })?;
            let value = lines[i + 1].trim();
            if code == 0 {
                records.push(Entity {
                    kind: value.to_uppercase(),
                    ..Entity::default()
                });
            } else if let Some(current) = records.last_mut() {
                current.pairs.push((code, value.to_owned()));
            }
            i += 2;
        }

        let mut section: Option<String> = None;
        let mut block: Option<String> = None;
        for r in records {
            if r.kind == "SECTION" {
                let s = r.string(2, "").to_uppercase();
                if s == "HEADER" {
                    doc.read_header(&r)?;
                }
                section = Some(s);
            } else if r.kind == "ENDSEC" {
                section = None;
            } else if section.as_deref() == Some("TABLES") && r.kind == "LAYER" {
                let aci = r.int(62, 7)?;
                let color = if r.has(420) {
                    true_color(r.int(420, 0)?)
                } else {
                    aci_color(aci.wrapping_abs())
                };
                let hidden = aci < 0 || (r.int(70, 0)? & 1) != 0;
                doc.layers
                    .insert(key(&r.string(2, "0")), DxfLayer { color, hidden });
            } else if section.as_deref() == Some("BLOCKS") {
                if r.kind == "BLOCK" {
                    let name = r.string(2, "");
                    let b = Block {
                        name: name.clone(),
                        base: (r.double(10, 0.0)?, r.double(20, 0.0)?),
                        entities: Vec::new(),
                    };
                    doc.blocks.insert(key(&name), b);
                    block = Some(key(&name));
                } else if r.kind == "ENDBLK" {
                    block = None;
                } else if let Some(b) = block.as_ref().and_then(|k| doc.blocks.get_mut(k)) {
                    add_entity(&mut b.entities, r);
                }
            } else if section.as_deref() == Some("ENTITIES") {
                add_entity(&mut doc.entities, r);
            }
        }
        Ok(doc)
    }

    fn read_header(&mut self, header: &Entity) -> Result<(), ImportError> {
        for w in header.pairs.windows(2) {
            if w[0].0 == 9 && w[0].1.to_uppercase() == "$INSUNITS" {
                self.units = parse_long(&w[1].1).ok_or_else(|| bad_number(&w[1].1))?;
                self.unit_to_mm = units_to_mm(self.units);
            }
        }
        Ok(())
    }
}

/// `VERTEX` records belong to the `POLYLINE` before them, until `SEQEND`.
fn add_entity(list: &mut Vec<Entity>, r: Entity) {
    let in_sequence = list
        .last()
        .is_some_and(|l| l.kind == "POLYLINE" && !l.sequence_ended);
    if r.kind == "VERTEX" && in_sequence {
        if let Some(last) = list.last_mut() {
            last.vertices.push(r);
        }
    } else if r.kind == "SEQEND" {
        if in_sequence && let Some(last) = list.last_mut() {
            last.sequence_ended = true;
        }
    } else if r.kind != "ATTRIB" {
        list.push(r);
    }
}

/// Affine transform `x' = A·x + C·y + E`, `y' = B·x + D·y + F`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Transform {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl Transform {
    const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    fn translation(x: f64, y: f64) -> Self {
        Self {
            e: x,
            f: y,
            ..Self::IDENTITY
        }
    }

    fn scaling(sx: f64, sy: f64) -> Self {
        Self {
            a: sx,
            d: sy,
            ..Self::IDENTITY
        }
    }

    fn rotation(degrees: f64) -> Self {
        let a = degrees * PI / 180.0;
        Self {
            a: a.cos(),
            b: a.sin(),
            c: -a.sin(),
            d: a.cos(),
            ..Self::IDENTITY
        }
    }

    /// Apply `other` first, then `self`.
    fn then(&self, o: &Self) -> Self {
        Self {
            a: self.a * o.a + self.c * o.b,
            b: self.b * o.a + self.d * o.b,
            c: self.a * o.c + self.c * o.d,
            d: self.b * o.c + self.d * o.d,
            e: self.a * o.e + self.c * o.f + self.e,
            f: self.b * o.e + self.d * o.f + self.f,
        }
    }

    fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (self.a * x + self.c * y + self.e, self.b * x + self.d * y + self.f)
    }

    fn determinant(&self) -> f64 {
        self.a * self.d - self.b * self.c
    }

    fn scale(&self) -> f64 {
        self.determinant().abs().sqrt()
    }

    /// Circles remain circles (uniform scale, rotation, mirror).
    fn is_similarity(&self) -> bool {
        let sx = self.a * self.a + self.b * self.b;
        let sy = self.c * self.c + self.d * self.d;
        (sx - sy).abs() <= 1e-9 * sx.max(sy) && (self.a * self.c + self.b * self.d).abs() <= 1e-9 * sx.max(sy)
    }
}

/// State inherited from the `INSERT` when drawing the entities of a block.
#[derive(Debug, Clone)]
struct Context {
    transform: Transform,
    block_layer: String,
    block_color: Color,
    depth: u32,
}

struct Builder<'d> {
    doc: &'d DxfDocument,
    paths: Vec<(Color, Path)>,
    path: Option<Path>,
    tolerance: f64,
    spline_tolerance: f64,
    count: usize,
    drawn: usize,
    segments_out: usize,
}

impl Builder<'_> {
    fn effective_layer(e: &Entity, ctx: &Context) -> String {
        let layer = e.string(8, "0");
        if layer == "0" && ctx.depth > 0 {
            ctx.block_layer.clone()
        } else {
            layer
        }
    }

    fn layer_color(&self, name: &str) -> Color {
        self.doc.layers.get(&key(name)).map_or(Color::BLACK, |l| l.color)
    }

    fn entity_color(&self, e: &Entity, ctx: &Context) -> Result<Color, ImportError> {
        if e.has(420) {
            return Ok(true_color(e.int(420, 0)?));
        }
        let aci = e.int(62, 256)?;
        Ok(match aci {
            0 => ctx.block_color,                                    // ByBlock
            256 => self.layer_color(&Self::effective_layer(e, ctx)), // ByLayer
            _ => aci_color(aci.wrapping_abs()),
        })
    }

    fn draw_entity(&mut self, e: &Entity, ctx: &Context) -> Result<(), ImportError> {
        self.drawn += 1;
        if self.drawn > MAX_DRAWN || self.segments_out > MAX_SEGMENTS {
            return Err(ImportError::Invalid(format!(
                "the drawing expands to more than {MAX_DRAWN} entities or {MAX_SEGMENTS} segments"
            )));
        }
        if e.int(60, 0)? == 1 {
            return Ok(()); // invisible
        }
        if self
            .doc
            .layers
            .get(&key(&Self::effective_layer(e, ctx)))
            .is_some_and(|l| l.hidden)
        {
            return Ok(());
        }
        let color = self.entity_color(e, ctx)?;
        let t = ctx.transform;
        // the object coordinate system of 2D entities is mirrored when the extrusion points down
        let ocs = if e.mirrored_ocs()? {
            t.then(&Transform::scaling(-1.0, 1.0))
        } else {
            t
        };
        self.tolerance = ARC_TOLERANCE / self.doc.unit_to_mm / t.scale().max(1e-9);

        match e.kind.as_str() {
            "LINE" => {
                self.begin();
                self.move_to(&t, e.double(10, 0.0)?, e.double(20, 0.0)?);
                self.line_to(&t, e.double(11, 0.0)?, e.double(21, 0.0)?);
                self.end(color, false);
            }
            "LWPOLYLINE" => {
                let mut v: Vec<[f64; 3]> = Vec::new();
                for (code, value) in &e.pairs {
                    let num = || parse_double(value).ok_or_else(|| bad_number(value));
                    match code {
                        10 => v.push([num()?, 0.0, 0.0]),
                        20 => {
                            if let Some(last) = v.last_mut() {
                                last[1] = num()?;
                            }
                        }
                        42 => {
                            if let Some(last) = v.last_mut() {
                                last[2] = num()?;
                            }
                        }
                        _ => {}
                    }
                }
                let closed = (e.int(70, 0)? & 1) != 0;
                self.vertices(&v, closed, &ocs, color);
            }
            "POLYLINE" => {
                let flags = e.int(70, 0)?;
                if flags & (16 | 64) != 0 {
                    return Ok(()); // polygon mesh, polyface mesh
                }
                let mut v = Vec::new();
                for vx in &e.vertices {
                    if vx.int(70, 0)? & 16 == 0 {
                        // skip spline frame control points
                        v.push([vx.double(10, 0.0)?, vx.double(20, 0.0)?, vx.double(42, 0.0)?]);
                    }
                }
                let is_3d = flags & 8 != 0; // world coordinates, no arcs
                self.vertices(&v, flags & 1 != 0, if is_3d { &t } else { &ocs }, color);
            }
            "CIRCLE" => {
                let r = e.double(40, 0.0)?;
                if r > 0.0 {
                    self.begin();
                    self.arc_to(
                        &ocs,
                        e.double(10, 0.0)?,
                        e.double(20, 0.0)?,
                        r,
                        0.0,
                        2.0 * PI,
                        true,
                    );
                    self.end(color, true);
                }
            }
            "ARC" => {
                let a1 = e.double(50, 0.0)?;
                let a2 = e.double(51, 360.0)?;
                let mut sweep = a2 - a1;
                while sweep <= 0.0 {
                    sweep += 360.0;
                }
                while sweep > 360.0 {
                    sweep -= 360.0;
                }
                self.begin();
                let (cx, cy, r) = (e.double(10, 0.0)?, e.double(20, 0.0)?, e.double(40, 0.0)?);
                self.arc_to(&ocs, cx, cy, r, a1 * PI / 180.0, sweep * PI / 180.0, true);
                self.end(color, false);
            }
            "ELLIPSE" => self.ellipse(e, &t, color)?,
            "SPLINE" => self.spline(e, &t, color)?,
            "INSERT" => return self.insert(e, ctx, color),
            _ => return Ok(()), // ignored
        }
        Ok(())
    }

    fn insert(&mut self, e: &Entity, ctx: &Context, color: Color) -> Result<(), ImportError> {
        let name = key(&e.string(2, ""));
        let Some(block) = self.doc.blocks.get(&name) else {
            return Ok(()); // block not found
        };
        if ctx.depth >= MAX_BLOCK_DEPTH {
            return Ok(()); // nested too deep
        }
        let parent = if e.mirrored_ocs()? {
            ctx.transform.then(&Transform::scaling(-1.0, 1.0))
        } else {
            ctx.transform
        };
        let position = Transform::translation(e.double(10, 0.0)?, e.double(20, 0.0)?)
            .then(&Transform::rotation(e.double(50, 0.0)?));
        let local = Transform::scaling(e.double(41, 1.0)?, e.double(42, 1.0)?)
            .then(&Transform::translation(-block.base.0, -block.base.1));
        // MINSERT: grid of copies, spacing along the rotated axes
        let cols = e.int(70, 1)?.max(1);
        let rows = e.int(71, 1)?.max(1);
        let (dx, dy) = (e.double(44, 0.0)?, e.double(45, 0.0)?);
        let _ = &block.name;
        for r in 0..rows {
            for c in 0..cols {
                let child = Context {
                    transform: parent
                        .then(&position)
                        .then(&Transform::translation(f64::from(c) * dx, f64::from(r) * dy))
                        .then(&local),
                    block_layer: Self::effective_layer(e, ctx),
                    block_color: color,
                    depth: ctx.depth + 1,
                };
                for be in &block.entities {
                    self.draw_entity(be, &child)?;
                }
            }
        }
        Ok(())
    }

    fn vertices(&mut self, v: &[[f64; 3]], closed: bool, t: &Transform, color: Color) {
        if v.is_empty() {
            return;
        }
        self.begin();
        self.move_to(t, v[0][0], v[0][1]);
        let segments = if closed { v.len() } else { v.len() - 1 };
        for i in 0..segments {
            let (a, b) = (v[i], v[(i + 1) % v.len()]);
            if a[2].abs() < 1e-9 {
                self.line_to(t, b[0], b[1]);
            } else {
                self.bulge_to(t, a[0], a[1], b[0], b[1], a[2]);
            }
        }
        self.end(color, closed);
    }

    /// An arc from (x1,y1) to (x2,y2): bulge is tan(angle/4), positive when counterclockwise.
    fn bulge_to(&mut self, t: &Transform, x1: f64, y1: f64, x2: f64, y2: f64, bulge: f64) {
        let (dx, dy) = (x2 - x1, y2 - y1);
        let chord = (dx * dx + dy * dy).sqrt();
        if chord < 1e-12 {
            return;
        }
        let angle = 4.0 * bulge.atan();
        let radius = chord / (2.0 * (angle.abs() / 2.0).sin());
        let offset = chord / 2.0 / (angle / 2.0).tan(); // signed distance of the center from the chord, on the left
        let cx = (x1 + x2) / 2.0 - dy / chord * offset;
        let cy = (y1 + y2) / 2.0 + dx / chord * offset;
        self.arc_to(t, cx, cy, radius, (y1 - cy).atan2(x1 - cx), angle, false);
    }

    fn ellipse(&mut self, e: &Entity, t: &Transform, color: Color) -> Result<(), ImportError> {
        let (cx, cy) = (e.double(10, 0.0)?, e.double(20, 0.0)?);
        let (mx, my) = (e.double(11, 0.0)?, e.double(21, 0.0)?);
        let ratio = e.double(40, 1.0)?;
        let nz = if e.double(230, 1.0)? < 0.0 { -1.0 } else { 1.0 };
        // minor axis = ratio * (extrusion x major axis)
        let (nx, ny) = (-nz * my * ratio, nz * mx * ratio);
        let p1 = e.double(41, 0.0)?;
        let p2 = e.double(42, 2.0 * PI)?;
        let mut sweep = p2 - p1;
        while sweep <= 1e-9 {
            sweep += 2.0 * PI;
        }
        while sweep > 2.0 * PI + 1e-9 {
            sweep -= 2.0 * PI;
        }
        let full = (sweep - 2.0 * PI).abs() < 1e-6;
        let major = (mx * mx + my * my).sqrt();
        let segments = self.segments(major, sweep);
        self.begin();
        for i in 0..=segments {
            if full && i == segments {
                break; // closed with Z
            }
            let a = p1 + sweep * f64::from(i) / f64::from(segments);
            let x = cx + a.cos() * mx + a.sin() * nx;
            let y = cy + a.cos() * my + a.sin() * ny;
            if i == 0 {
                self.move_to(t, x, y);
            } else {
                self.line_to(t, x, y);
            }
        }
        self.end(color, full);
        Ok(())
    }

    fn spline(&mut self, e: &Entity, t: &Transform, color: Color) -> Result<(), ImportError> {
        let degree = e.int(71, 3)?;
        let knots = e.all(40)?;
        let mut weights = e.all(41)?;
        let (xs, ys) = (e.all(10)?, e.all(20)?);
        let closed = (e.int(70, 0)? & 1) != 0;
        let p = usize::try_from(degree).unwrap_or(usize::MAX);
        let mut points: Vec<(f64, f64)> = Vec::new();
        if p != usize::MAX && xs.len() > p && ys.len() == xs.len() && knots.len() == xs.len() + p + 1 {
            if weights.len() != xs.len() {
                weights = vec![1.0; xs.len()];
            }
            points = self.evaluate_spline(p, &knots, &weights, &xs, &ys, t.scale());
        } else {
            // no usable control points: go through the fit points
            let (fx, fy) = (e.all(11)?, e.all(21)?);
            points.extend(fx.iter().zip(fy.iter()).map(|(x, y)| (*x, *y)));
        }
        if points.len() < 2 {
            return Ok(()); // invalid spline
        }
        self.begin();
        self.move_to(t, points[0].0, points[0].1);
        if self.spline_tolerance > 0.0 {
            // fitted on the final coordinates: correct also inside scaled or rotated blocks
            let drawing: Vec<Point> = points.iter().map(|q| self.to_drawing(t, q.0, q.1)).collect();
            let segs = arcfit::fit(&drawing, self.spline_tolerance);
            if let Some(path) = &mut self.path {
                path.segments.extend(segs);
            }
        } else {
            for q in &points[1..] {
                self.line_to(t, q.0, q.1);
            }
        }
        self.end(color, closed);
        Ok(())
    }

    /// Samples a (rational) B-spline span by span, with segments of about 0.1 mm.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn evaluate_spline(
        &self,
        p: usize,
        knots: &[f64],
        w: &[f64],
        xs: &[f64],
        ys: &[f64],
        scale: f64,
    ) -> Vec<(f64, f64)> {
        let mut rv = Vec::new();
        let n = xs.len();
        for span in p..n {
            let (u0, u1) = (knots[span], knots[span + 1]);
            if u1 - u0 <= 1e-12 {
                continue;
            }
            // the curve in this span is inside the control polygon of the p+1 points that define it
            let mut len = 0.0;
            for k in span - p..span {
                len += ((xs[k + 1] - xs[k]).powi(2) + (ys[k + 1] - ys[k]).powi(2)).sqrt();
            }
            let segments =
                ((len * scale * self.doc.unit_to_mm / SPLINE_STEP).ceil() as i64).clamp(4, 10000) as i32;
            let first = i32::from(!rv.is_empty());
            for i in first..=segments {
                rv.push(de_boor(
                    p,
                    knots,
                    w,
                    xs,
                    ys,
                    span,
                    u0 + (u1 - u0) * f64::from(i) / f64::from(segments),
                ));
            }
        }
        rv
    }

    fn begin(&mut self) {
        self.path = None;
    }

    /// Adds a segment to the current path, within the segment budget.
    fn push(&mut self, seg: Segment) {
        if self.segments_out > MAX_SEGMENTS {
            return;
        }
        if let Some(path) = &mut self.path {
            path.segments.push(seg);
            self.segments_out += 1;
        }
    }

    fn end(&mut self, color: Color, closed: bool) {
        let Some(mut path) = self.path.take() else {
            return;
        };
        if path.segments.is_empty() {
            return;
        }
        if closed && path.end().distance(path.start) > 1e-9 {
            let start = path.start;
            path.segments.push(Segment::line(start));
        }
        path.closed = closed;
        self.paths.push((color, path));
        self.count += 1;
    }

    /// From local coordinates to the drawing: block transformations, then units to mm.
    fn to_drawing(&self, t: &Transform, x: f64, y: f64) -> Point {
        let (px, py) = t.apply(x, y);
        Point::new(px * self.doc.unit_to_mm, py * self.doc.unit_to_mm)
    }

    fn move_to(&mut self, t: &Transform, x: f64, y: f64) {
        self.path = Some(Path::new(self.to_drawing(t, x, y)));
    }

    fn line_to(&mut self, t: &Transform, x: f64, y: f64) {
        let p = self.to_drawing(t, x, y);
        self.push(Segment::line(p));
    }

    /// A circular arc in local coordinates, sweep in radians (positive = counterclockwise).
    #[allow(clippy::too_many_arguments)]
    fn arc_to(&mut self, t: &Transform, cx: f64, cy: f64, r: f64, start: f64, sweep: f64, mv: bool) {
        if r <= 0.0 {
            return;
        }
        if mv {
            self.move_to(t, cx + r * start.cos(), cy + r * start.sin());
        }
        if t.is_similarity() {
            // a true arc (mirrored blocks invert the direction); a full circle in two halves
            let ccw = (sweep > 0.0) ^ (t.determinant() < 0.0);
            let center = self.to_drawing(t, cx, cy);
            let pieces = if sweep.abs() > 2.0 * PI - 1e-9 { 2 } else { 1 };
            for i in 1..=pieces {
                let a = start + sweep * f64::from(i) / f64::from(pieces);
                let end = self.to_drawing(t, cx + r * a.cos(), cy + r * a.sin());
                self.push(Segment::arc(end, center, ccw));
            }
        } else {
            let segments = self.segments(r, sweep.abs());
            for i in 1..=segments {
                let a = start + sweep * f64::from(i) / f64::from(segments);
                self.line_to(t, cx + r * a.cos(), cy + r * a.sin());
            }
        }
    }

    /// Segments needed to approximate an arc of `radius` (drawing units) within the tolerance.
    #[allow(clippy::cast_possible_truncation)]
    fn segments(&self, radius: f64, sweep: f64) -> i32 {
        let step = if radius > self.tolerance {
            2.0 * (1.0 - self.tolerance / radius).acos()
        } else {
            PI / 4.0
        };
        let n = (sweep / step.min(PI / 4.0)).ceil();
        if n.is_nan() {
            4
        } else {
            (n.min(10000.0) as i32).max(4)
        }
    }
}

/// De Boor's algorithm in homogeneous coordinates.
fn de_boor(p: usize, knots: &[f64], w: &[f64], xs: &[f64], ys: &[f64], span: usize, u: f64) -> (f64, f64) {
    let mut d: Vec<[f64; 3]> = (0..=p)
        .map(|j| {
            let i = j + span - p;
            [xs[i] * w[i], ys[i] * w[i], w[i]]
        })
        .collect();
    for r in 1..=p {
        for j in (r..=p).rev() {
            let i = j + span - p;
            let den = knots[i + p - r + 1] - knots[i];
            let alpha = if den == 0.0 { 0.0 } else { (u - knots[i]) / den };
            let prev = d[j - 1];
            for (v, q) in d[j].iter_mut().zip(prev) {
                *v = (1.0 - alpha) * q + alpha * *v;
            }
        }
    }
    (d[p][0] / d[p][2], d[p][1] / d[p][2])
}

/// Imports an ASCII DXF file.
///
/// # Errors
/// A binary DXF, malformed group codes or numbers, or nothing to import.
pub fn import(bytes: &[u8], options: &DxfOptions) -> Result<Import, ImportError> {
    if bytes.starts_with(b"AutoCAD Binary DXF") {
        return Err(ImportError::BinaryDxf);
    }
    let text = decode_text(bytes);
    let doc = DxfDocument::load(&lines(&text))?;
    let mut b = Builder {
        doc: &doc,
        paths: Vec::new(),
        path: None,
        tolerance: 0.0,
        spline_tolerance: options.spline_tolerance,
        count: 0,
        drawn: 0,
        segments_out: 0,
    };
    let ctx = Context {
        transform: Transform::IDENTITY,
        block_layer: "0".into(),
        block_color: Color::BLACK,
        depth: 0,
    };
    for e in &doc.entities {
        if e.int(67, 0)? == 0 {
            // skip paper space
            b.draw_entity(e, &ctx)?;
        }
    }
    if b.count == 0 {
        return Err(ImportError::Empty);
    }
    let mut paths = b.paths;
    // move the drawing so that its bounds start at 0,0 (as done for the svg files)
    let mut r = Rect::EMPTY;
    for (_, p) in &paths {
        r.union(&p.bounds());
    }
    if !r.is_empty() {
        for (_, p) in &mut paths {
            p.translate(-r.min_x, -r.min_y);
        }
    }
    let mut colors: Vec<ColorCount> = Vec::new();
    for (c, _) in &paths {
        match colors.iter_mut().find(|x| x.color == *c) {
            Some(x) => x.count += 1,
            None => colors.push(ColorCount { color: *c, count: 1 }),
        }
    }
    Ok(Import {
        colors,
        objects: paths
            .into_iter()
            .map(|(color, p)| ImportedObject::new(color, ObjectKind::Shape { paths: vec![p] }))
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dxf(entities: &str) -> Vec<u8> {
        format!(
            "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nTABLES\n0\nLAYER\n2\nRED\n62\n1\n0\nLAYER\n2\nOFF\n62\n-3\n0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nB\n10\n0\n20\n0\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n1\n21\n0\n0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n0\nEOF\n"
        )
        .into_bytes()
    }

    #[test]
    fn blocks_inside_themselves_and_huge_grids() {
        // B inserts itself twice: 2^32 copies at LaserGRBL's depth limit, refused
        let looped = "0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nB\n10\n0\n20\n0\n\
                      0\nLINE\n10\n0\n20\n0\n11\n1\n21\n0\n\
                      0\nINSERT\n2\nB\n10\n1\n20\n0\n0\nINSERT\n2\nB\n10\n0\n20\n1\n\
                      0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nINSERT\n2\nB\n10\n0\n20\n0\n\
                      0\nENDSEC\n0\nEOF\n";
        assert!(matches!(
            import(looped.as_bytes(), &DxfOptions::default()),
            Err(ImportError::Invalid(_))
        ));
        // a MINSERT grid of 2000 × 2000 copies is refused
        let grid = "0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nG\n10\n0\n20\n0\n\
                    0\nLINE\n10\n0\n20\n0\n11\n1\n21\n0\n0\nENDBLK\n0\nENDSEC\n\
                    0\nSECTION\n2\nENTITIES\n0\nINSERT\n2\nG\n10\n0\n20\n0\n70\n2000\n71\n2000\n44\n1\n45\n1\n\
                    0\nENDSEC\n0\nEOF\n";
        assert!(matches!(
            import(grid.as_bytes(), &DxfOptions::default()),
            Err(ImportError::Invalid(_))
        ));
    }

    #[test]
    fn lines_arcs_layers_and_blocks() {
        let bytes = dxf(concat!(
            "0\nLINE\n8\nRED\n10\n10\n20\n10\n11\n20\n21\n10\n",
            "0\nCIRCLE\n8\n0\n62\n5\n10\n0\n20\n0\n40\n2\n",
            "0\nLINE\n8\nOFF\n10\n0\n20\n0\n11\n1\n21\n1\n",
            "0\nINSERT\n8\nRED\n2\nB\n10\n5\n20\n5\n41\n2\n42\n2\n",
            "0\nLWPOLYLINE\n70\n1\n10\n0\n20\n0\n42\n1\n10\n4\n20\n0\n",
            "0\nARC\n10\n0\n20\n0\n40\n1\n50\n0\n51\n90\n",
            "0\nELLIPSE\n10\n0\n20\n0\n11\n2\n21\n0\n40\n0.5\n",
            "0\nPOLYLINE\n70\n0\n0\nVERTEX\n10\n0\n20\n0\n0\nVERTEX\n10\n1\n20\n1\n0\nSEQEND\n",
            "0\nTEXT\n1\nignored\n",
        ));
        let imp = import(&bytes, &DxfOptions::default()).unwrap();
        let colors: Vec<String> = imp
            .colors
            .iter()
            .map(|c| format!("{}x{}", c.color, c.count))
            .collect();
        assert_eq!(colors, ["#FF0000x2", "#0000FFx1", "#000000x4"]);
        // the circle is two half arcs
        let ObjectKind::Shape { paths } = &imp.objects[1].kind else {
            panic!("shape")
        };
        assert_eq!(paths[0].segments.len(), 2);
        assert!(paths[0].closed);
        // moved to the origin
        let mut r = Rect::EMPTY;
        for o in &imp.objects {
            if let ObjectKind::Shape { paths } = &o.kind {
                for p in paths {
                    r.union(&p.bounds());
                }
            }
        }
        assert!(r.min_x.abs() < 1e-9 && r.min_y.abs() < 1e-9);
    }

    #[test]
    fn splines_units_and_errors() {
        let spline = "0\nSPLINE\n71\n3\n40\n0\n40\n0\n40\n0\n40\n0\n40\n1\n40\n1\n40\n1\n40\n1\n10\n0\n20\n0\n10\n1\n20\n2\n10\n3\n20\n2\n10\n4\n20\n0\n";
        let arcs = import(&dxf(spline), &DxfOptions::default()).unwrap();
        let lines = import(
            &dxf(spline),
            &DxfOptions {
                spline_tolerance: 0.0,
            },
        )
        .unwrap();
        let count = |i: &Import| match &i.objects[0].kind {
            ObjectKind::Shape { paths } => paths[0].segments.len(),
            _ => 0,
        };
        assert!(count(&arcs) < count(&lines));
        let fit = "0\nSPLINE\n11\n0\n21\n0\n11\n1\n21\n1\n";
        assert_eq!(count(&import(&dxf(fit), &DxfOptions::default()).unwrap()), 1);
        assert_eq!(
            import(b"AutoCAD Binary DXF\r\n\x1a\0", &DxfOptions::default()),
            Err(ImportError::BinaryDxf)
        );
        assert_eq!(import(&dxf(""), &DxfOptions::default()), Err(ImportError::Empty));
        assert!(import(b"x\nSECTION\n", &DxfOptions::default()).is_err());
        assert!(import(&dxf("0\nLINE\n10\nabc\n"), &DxfOptions::default()).is_err());
        assert_eq!(lines_of("a\r\nb\rc\n"), vec!["a", "b", "c"]);
        assert_eq!(units_to_mm(1), 25.4);
        assert_eq!(aci_color(7), Color::BLACK);
        assert_eq!(aci_color(1), Color::rgb(255, 0, 0));
        assert_eq!(aci_color(300), Color::BLACK);
    }

    fn lines_of(s: &str) -> Vec<&str> {
        lines(s)
    }
}
