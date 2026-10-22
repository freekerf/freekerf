//! SVG path data (`d`) as [`Path`]s: every command, relative and absolute, with
//! quadratic curves raised to cubics and elliptical arcs approximated by cubics.
//! The glyphs of SVG fonts are written this way. Coordinates are kept as they are
//! (the caller scales and flips them). Parsing stops at the first error and keeps
//! what came before it, as SVG renderers do.

use std::f64::consts::{FRAC_PI_2, PI};

use freekerf_geometry::{Path, Point, Segment};

struct Cursor<'a> {
    s: &'a [u8],
    i: usize,
}

impl Cursor<'_> {
    fn skip_separators(&mut self) {
        while self.i < self.s.len() && (self.s[self.i].is_ascii_whitespace() || self.s[self.i] == b',') {
            self.i += 1;
        }
    }

    fn at_number(&mut self) -> bool {
        self.skip_separators();
        self.s
            .get(self.i)
            .is_some_and(|c| c.is_ascii_digit() || matches!(c, b'-' | b'+' | b'.'))
    }

    fn number(&mut self) -> Option<f64> {
        self.skip_separators();
        let start = self.i;
        let digits = |c: &mut Self| {
            let from = c.i;
            while c.i < c.s.len() && c.s[c.i].is_ascii_digit() {
                c.i += 1;
            }
            c.i > from
        };
        if matches!(self.s.get(self.i), Some(b'-' | b'+')) {
            self.i += 1;
        }
        let mut any = digits(self);
        if self.s.get(self.i) == Some(&b'.') {
            self.i += 1;
            any |= digits(self);
        }
        if !any {
            self.i = start;
            return None;
        }
        if matches!(self.s.get(self.i), Some(b'e' | b'E')) {
            let mark = self.i;
            self.i += 1;
            if matches!(self.s.get(self.i), Some(b'-' | b'+')) {
                self.i += 1;
            }
            if !digits(self) {
                self.i = mark;
            }
        }
        std::str::from_utf8(&self.s[start..self.i]).ok()?.parse().ok()
    }

    /// An arc flag: one `0` or `1`, separators optional (`a1 1 0 01 2 3`).
    fn flag(&mut self) -> Option<bool> {
        self.skip_separators();
        let c = *self.s.get(self.i)?;
        self.i += 1;
        match c {
            b'0' => Some(false),
            b'1' => Some(true),
            _ => None,
        }
    }

    fn command(&mut self) -> Option<u8> {
        self.skip_separators();
        let c = *self.s.get(self.i)?;
        c.is_ascii_alphabetic().then(|| {
            self.i += 1;
            c
        })
    }

    fn at_end(&mut self) -> bool {
        self.skip_separators();
        self.i >= self.s.len()
    }
}

#[derive(Default)]
struct Builder {
    out: Vec<Path>,
    current: Option<Path>,
    pos: Point,
    start: Point,
    /// The second control point of the last cubic (`S`) or the control point of
    /// the last quadratic (`T`), for the smooth commands.
    last_cubic: Option<Point>,
    last_quad: Option<Point>,
}

impl Builder {
    fn finish(&mut self) {
        if let Some(p) = self.current.take()
            && !p.segments.is_empty()
        {
            self.out.push(p);
        }
    }

    fn move_to(&mut self, p: Point) {
        self.finish();
        self.current = Some(Path::new(p));
        self.pos = p;
        self.start = p;
    }

    fn path(&mut self) -> &mut Path {
        let pos = self.pos;
        self.current.get_or_insert_with(|| Path::new(pos))
    }

    fn line_to(&mut self, p: Point) {
        self.path().segments.push(Segment::line(p));
        self.pos = p;
    }

    fn cubic_to(&mut self, c1: Point, c2: Point, end: Point) {
        self.path().segments.push(Segment::Cubic { c1, c2, end });
        self.pos = end;
        self.last_cubic = Some(c2);
    }

    fn quad_to(&mut self, c: Point, end: Point) {
        let p0 = self.pos;
        let c1 = p0 + (c - p0) * (2.0 / 3.0);
        let c2 = end + (c - end) * (2.0 / 3.0);
        self.cubic_to(c1, c2, end);
        self.last_quad = Some(c);
    }

    fn close(&mut self) {
        let start = self.start;
        if let Some(p) = &mut self.current {
            if p.end() != start {
                p.segments.push(Segment::line(start));
            }
            p.closed = true;
        }
        self.finish();
        self.pos = start;
    }

    /// An elliptical arc (SVG implementation notes F.6.5) as cubics of at most a
    /// quarter turn each.
    #[allow(clippy::too_many_arguments, clippy::many_single_char_names)]
    fn arc_to(&mut self, rx: f64, ry: f64, rotation_deg: f64, large: bool, sweep: bool, end: Point) {
        let p0 = self.pos;
        let (mut rx, mut ry) = (rx.abs(), ry.abs());
        if p0 == end {
            return;
        }
        if rx == 0.0 || ry == 0.0 {
            self.line_to(end);
            return;
        }
        let phi = rotation_deg.to_radians();
        let (sin, cos) = phi.sin_cos();
        let dx = (p0.x - end.x) / 2.0;
        let dy = (p0.y - end.y) / 2.0;
        let x1 = cos * dx + sin * dy;
        let y1 = -sin * dx + cos * dy;
        let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
        if lambda > 1.0 {
            let k = lambda.sqrt();
            rx *= k;
            ry *= k;
        }
        let num = (rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1).max(0.0);
        let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
        let mut coef = if den > 0.0 { (num / den).sqrt() } else { 0.0 };
        if large == sweep {
            coef = -coef;
        }
        let cxp = coef * rx * y1 / ry;
        let cyp = -coef * ry * x1 / rx;
        let cx = cos * cxp - sin * cyp + (p0.x + end.x) / 2.0;
        let cy = sin * cxp + cos * cyp + (p0.y + end.y) / 2.0;
        let angle = |ux: f64, uy: f64, vx: f64, vy: f64| {
            let a = (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
            if a.is_nan() { 0.0 } else { a }
        };
        let theta1 = angle(1.0, 0.0, (x1 - cxp) / rx, (y1 - cyp) / ry);
        let mut delta = angle(
            (x1 - cxp) / rx,
            (y1 - cyp) / ry,
            (-x1 - cxp) / rx,
            (-y1 - cyp) / ry,
        );
        if !sweep && delta > 0.0 {
            delta -= 2.0 * PI;
        } else if sweep && delta < 0.0 {
            delta += 2.0 * PI;
        }
        let pieces = (delta.abs() / FRAC_PI_2).ceil().max(1.0);
        let step = delta / pieces;
        let k = 4.0 / 3.0 * (step / 4.0).tan();
        let point = |t: f64| {
            let (st, ct) = t.sin_cos();
            Point::new(
                cx + rx * ct * cos - ry * st * sin,
                cy + rx * ct * sin + ry * st * cos,
            )
        };
        let deriv = |t: f64| {
            let (st, ct) = t.sin_cos();
            Point::new(-rx * st * cos - ry * ct * sin, -rx * st * sin + ry * ct * cos)
        };
        let mut t = theta1;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        for i in 0..pieces as usize {
            let t2 = t + step;
            let a = point(t);
            let b = if i + 1 == pieces as usize { end } else { point(t2) };
            self.cubic_to(a + deriv(t) * k, b - deriv(t2) * k, b);
            t = t2;
        }
    }
}

/// The paths of SVG path data `d`.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn parse(d: &str) -> Vec<Path> {
    let mut c = Cursor {
        s: d.as_bytes(),
        i: 0,
    };
    let mut b = Builder::default();
    let mut cmd = 0u8;
    loop {
        if c.at_end() {
            break;
        }
        if let Some(next) = c.command() {
            cmd = next;
        } else if cmd == 0 || !c.at_number() {
            break;
        }
        let rel = cmd.is_ascii_lowercase();
        let origin = if rel { b.pos } else { Point::ZERO };
        let pt = |c: &mut Cursor<'_>| Some(Point::new(c.number()?, c.number()?));
        let ok = match cmd.to_ascii_uppercase() {
            b'Z' => {
                b.close();
                b.last_cubic = None;
                b.last_quad = None;
                // a number after Z is an error
                if c.at_number() {
                    break;
                }
                continue;
            }
            b'M' => pt(&mut c).map(|p| {
                b.move_to(origin + p);
                // following pairs are lines
                cmd = if rel { b'l' } else { b'L' };
            }),
            b'L' => pt(&mut c).map(|p| b.line_to(origin + p)),
            b'H' => c.number().map(|x| {
                let y = b.pos.y;
                b.line_to(Point::new(if rel { b.pos.x + x } else { x }, y));
            }),
            b'V' => c.number().map(|y| {
                let x = b.pos.x;
                b.line_to(Point::new(x, if rel { b.pos.y + y } else { y }));
            }),
            b'C' => (|| Some((pt(&mut c)?, pt(&mut c)?, pt(&mut c)?)))()
                .map(|(c1, c2, e)| b.cubic_to(origin + c1, origin + c2, origin + e)),
            b'S' => (|| Some((pt(&mut c)?, pt(&mut c)?)))().map(|(c2, e)| {
                let c1 = b.last_cubic.map_or(b.pos, |l| b.pos * 2.0 - l);
                b.cubic_to(c1, origin + c2, origin + e);
            }),
            b'Q' => (|| Some((pt(&mut c)?, pt(&mut c)?)))().map(|(q, e)| b.quad_to(origin + q, origin + e)),
            b'T' => pt(&mut c).map(|e| {
                let q = b.last_quad.map_or(b.pos, |l| b.pos * 2.0 - l);
                b.quad_to(q, origin + e);
            }),
            b'A' => (|| {
                let rx = c.number()?;
                let ry = c.number()?;
                let rot = c.number()?;
                let large = c.flag()?;
                let sweep = c.flag()?;
                Some((rx, ry, rot, large, sweep, pt(&mut c)?))
            })()
            .map(|(rx, ry, rot, large, sweep, e)| b.arc_to(rx, ry, rot, large, sweep, origin + e)),
            _ => None,
        };
        if ok.is_none() {
            break;
        }
        // the smooth commands only reflect a control point of the same kind
        let upper = cmd.to_ascii_uppercase();
        if !matches!(upper, b'C' | b'S') {
            b.last_cubic = None;
        }
        if !matches!(upper, b'Q' | b'T') {
            b.last_quad = None;
        }
    }
    b.finish();
    b.out
}

fn num(out: &mut String, v: f64) {
    use std::fmt::Write;
    let r = (v * 1000.0).round() / 1000.0;
    let r = if r == 0.0 { 0.0 } else { r };
    let _ = write!(out, "{r}");
}

fn pair(out: &mut String, p: Point, flip_y: bool) {
    num(out, p.x);
    out.push(' ');
    num(out, if flip_y { -p.y } else { p.y });
}

/// SVG path data of `paths` (`flip_y`: Y up becomes Y down, for screens), to the
/// micrometre.
#[must_use]
pub fn to_data(paths: &[Path], flip_y: bool) -> String {
    let mut out = String::new();
    for p in paths {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push('M');
        pair(&mut out, p.start, flip_y);
        let mut at = p.start;
        for s in &p.segments {
            match *s {
                Segment::Line { end } => {
                    out.push_str(" L");
                    pair(&mut out, end, flip_y);
                }
                Segment::Cubic { c1, c2, end } => {
                    out.push_str(" C");
                    pair(&mut out, c1, flip_y);
                    out.push(' ');
                    pair(&mut out, c2, flip_y);
                    out.push(' ');
                    pair(&mut out, end, flip_y);
                }
                Segment::Arc { end, center, ccw } => {
                    let sweep = freekerf_geometry::arc_sweep(at, end, center, ccw);
                    let r = end.distance(center);
                    out.push_str(" A");
                    num(&mut out, r);
                    out.push(' ');
                    num(&mut out, r);
                    // a counterclockwise arc, Y up, turns clockwise on a Y-down screen
                    let positive = ccw != flip_y;
                    out.push_str(if sweep.abs() > PI { " 0 1 " } else { " 0 0 " });
                    out.push_str(if positive { "1 " } else { "0 " });
                    pair(&mut out, end, flip_y);
                }
            }
            at = s.end();
        }
        if p.closed {
            out.push_str(" Z");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Point, b: Point) -> bool {
        a.distance(b) < 1e-9
    }

    #[test]
    fn lines_moves_and_closing() {
        let p = parse("M334 22.1 L416 34.6 L498 88.2 M 1,2 3,4 z");
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].segments.len(), 2);
        assert!(close(p[0].end(), Point::new(498.0, 88.2)));
        assert!(p[1].closed && close(p[1].end(), Point::new(1.0, 2.0)));
        // relative commands, packed numbers and exponents
        let p = parse("m10-5h2v3l-1-1.5.5.5e1z");
        assert!(close(p[0].start, Point::new(10.0, -5.0)));
        assert!(close(p[0].segments[0].end(), Point::new(12.0, -5.0)));
        assert!(close(p[0].segments[1].end(), Point::new(12.0, -2.0)));
        assert!(close(p[0].segments[2].end(), Point::new(11.0, -3.5)));
        assert!(close(p[0].segments[3].end(), Point::new(11.5, 1.5)));
        assert!(p[0].closed);
    }

    #[test]
    fn curves() {
        // Relief SingleLine writes relative cubics, smooth cubics and lines
        let p = parse("M566 537c-37 94 -106 143 -208 143s-263 -124 -263 -340");
        let Segment::Cubic { c1, .. } = p[0].segments[1] else {
            panic!("{p:?}")
        };
        // the reflection of the previous second control point (460, 680) about (358, 680)
        assert!(close(c1, Point::new(256.0, 680.0)), "{c1:?}");
        assert!(close(p[0].end(), Point::new(95.0, 340.0)));
        // a quadratic becomes the same cubic
        let q = parse("M0 0 Q 3 3 6 0 T 12 0");
        let Segment::Cubic { c1, c2, end } = q[0].segments[0] else {
            panic!()
        };
        assert!(
            close(c1, Point::new(2.0, 2.0))
                && close(c2, Point::new(4.0, 2.0))
                && close(end, Point::new(6.0, 0.0))
        );
        let Segment::Cubic { c1, .. } = q[0].segments[1] else {
            panic!()
        };
        assert!(close(c1, Point::new(8.0, -2.0)));
    }

    #[test]
    fn arcs() {
        // half a circle of radius 5, as one or two cubics, ends where asked
        let p = parse("M0 0 A5 5 0 0 1 10 0");
        assert!(close(p[0].end(), Point::new(10.0, 0.0)));
        assert_eq!(p[0].segments.len(), 2);
        let mid = p[0].segments[0].end();
        assert!((mid.distance(Point::new(5.0, 0.0)) - 5.0).abs() < 1e-9, "{mid:?}");
        // packed flags, radii too small are scaled up, zero radius is a line
        let p = parse("M0 0a1 1 0 0110 0");
        assert!(close(p[0].end(), Point::new(10.0, 0.0)));
        let p = parse("M0 0 A0 5 0 0 1 10 0");
        assert!(matches!(p[0].segments[0], Segment::Line { .. }));
    }

    #[test]
    fn paths_back_to_data() {
        let p = parse("M0 0 L10 0 C 10 5 5 10 0 10 Z");
        assert_eq!(to_data(&p, false), "M0 0 L10 0 C10 5 5 10 0 10 L0 0 Z");
        assert_eq!(to_data(&p, true), "M0 0 L10 0 C10 -5 5 -10 0 -10 L0 0 Z");
        let mut arc = Path::new(Point::new(1.0, 0.0));
        arc.segments
            .push(Segment::arc(Point::new(0.0, 1.0), Point::ZERO, true));
        assert_eq!(to_data(&[arc.clone()], false), "M1 0 A1 1 0 0 1 0 1");
        assert_eq!(to_data(&[arc], true), "M1 0 A1 1 0 0 0 0 -1");
        // and parsing it again gives the same geometry
        let back = parse(&to_data(&parse("M0 0 L1.5 2.25"), false));
        assert!(close(back[0].end(), Point::new(1.5, 2.25)));
    }

    #[test]
    fn errors_keep_what_came_before() {
        assert!(parse("").is_empty());
        assert!(parse("L 1 2").len() == 1);
        let p = parse("M0 0 L1 1 L2 x");
        assert_eq!(p[0].segments.len(), 1);
        assert!(parse("M 0 0 Z 5").is_empty());
        assert!(parse("12 3").is_empty());
    }
}
