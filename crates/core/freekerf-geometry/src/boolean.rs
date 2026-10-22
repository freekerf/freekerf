//! Boolean operations and offsetting of polygons in floating point: a thin layer
//! over [`crate::clipper`] that scales to an integer grid and back.

use crate::Point;
use crate::clipper::{
    ClipType, Clipper, ClipperError, ClipperOffset, EndType, IntPoint, JoinType, Paths, PolyFillType,
    PolyType,
};

/// Default grid: 1 unit = 10 000 integer steps (0.1 µm when the unit is the mm).
pub const DEFAULT_SCALE: f64 = 10_000.0;

/// A boolean operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// Area in both.
    Intersection,
    /// Area in either.
    Union,
    /// Area in the first and not in the second.
    Difference,
    /// Area in exactly one.
    Xor,
}

/// How offsets treat corners.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Join {
    /// Square corners.
    Square,
    /// Round corners within `arc_tolerance` units.
    Round {
        /// Maximum deviation of the arc approximation.
        arc_tolerance: f64,
    },
    /// Mitered corners up to `limit` × the offset.
    Miter {
        /// Miter limit.
        limit: f64,
    },
}

#[allow(clippy::cast_possible_truncation)]
fn to_int(polys: &[Vec<Point>], scale: f64) -> Paths {
    polys
        .iter()
        .map(|p| {
            p.iter()
                .map(|q| IntPoint::new((q.x * scale).round() as i64, (q.y * scale).round() as i64))
                .collect()
        })
        .collect()
}

#[allow(clippy::cast_precision_loss)]
fn from_int(paths: &Paths, scale: f64) -> Vec<Vec<Point>> {
    paths
        .iter()
        .map(|p| {
            p.iter()
                .map(|q| Point::new(q.x as f64 / scale, q.y as f64 / scale))
                .collect()
        })
        .collect()
}

/// `subject op clip` with the non-zero rule; polygons are implicitly closed.
///
/// # Errors
/// Coordinates out of range at `scale`.
pub fn boolean(
    subject: &[Vec<Point>],
    clip: &[Vec<Point>],
    op: Op,
    scale: f64,
) -> Result<Vec<Vec<Point>>, ClipperError> {
    let mut c = Clipper::new();
    c.add_paths(&to_int(subject, scale), PolyType::Subject, true)?;
    c.add_paths(&to_int(clip, scale), PolyType::Clip, true)?;
    let ct = match op {
        Op::Intersection => ClipType::Intersection,
        Op::Union => ClipType::Union,
        Op::Difference => ClipType::Difference,
        Op::Xor => ClipType::Xor,
    };
    let out = c.execute(ct, PolyFillType::NonZero, PolyFillType::NonZero)?;
    Ok(out.map(|p| from_int(&p, scale)).unwrap_or_default())
}

/// Grows (`delta > 0`) or shrinks closed polygons by `delta` units.
///
/// # Errors
/// Coordinates out of range at `scale`.
pub fn offset(
    polys: &[Vec<Point>],
    delta: f64,
    join: Join,
    scale: f64,
) -> Result<Vec<Vec<Point>>, ClipperError> {
    let (jt, miter, arc) = match join {
        Join::Square => (JoinType::Square, 2.0, 0.0),
        Join::Round { arc_tolerance } => (JoinType::Round, 2.0, arc_tolerance * scale),
        Join::Miter { limit } => (JoinType::Miter, limit, 0.0),
    };
    let mut o = ClipperOffset::new(miter, arc);
    o.add_paths(&to_int(polys, scale), jt, EndType::ClosedPolygon);
    Ok(from_int(&o.execute(delta * scale)?, scale))
}

/// The outline of open polylines thickened by `delta` units on each side, with
/// round ends (a stroke drawn with a pen of width `2 × delta`).
///
/// # Errors
/// Coordinates out of range at `scale`.
pub fn offset_open(
    lines: &[Vec<Point>],
    delta: f64,
    arc_tolerance: f64,
    scale: f64,
) -> Result<Vec<Vec<Point>>, ClipperError> {
    let mut o = ClipperOffset::new(2.0, arc_tolerance * scale);
    o.add_paths(&to_int(lines, scale), JoinType::Round, EndType::OpenRound);
    Ok(from_int(&o.execute(delta.abs() * scale)?, scale))
}

/// Signed area of a polygon (positive counterclockwise, Y up).
#[must_use]
pub fn signed_area(poly: &[Point]) -> f64 {
    let n = poly.len();
    if n < 3 {
        return 0.0;
    }
    let mut a = 0.0;
    for i in 0..n {
        let (p, q) = (poly[i], poly[(i + 1) % n]);
        a += p.x * q.y - q.x * p.y;
    }
    a / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sq(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Point> {
        vec![
            Point::new(x0, y0),
            Point::new(x1, y0),
            Point::new(x1, y1),
            Point::new(x0, y1),
        ]
    }

    fn total(polys: &[Vec<Point>]) -> f64 {
        polys.iter().map(|p| signed_area(p)).sum()
    }

    #[test]
    fn operations_on_overlapping_squares() {
        let a = [sq(0.0, 0.0, 2.0, 2.0)];
        let b = [sq(1.0, 1.0, 3.0, 3.0)];
        let s = DEFAULT_SCALE;
        assert!((total(&boolean(&a, &b, Op::Intersection, s).unwrap()).abs() - 1.0).abs() < 1e-9);
        assert!((total(&boolean(&a, &b, Op::Union, s).unwrap()).abs() - 7.0).abs() < 1e-9);
        assert!((total(&boolean(&a, &b, Op::Difference, s).unwrap()).abs() - 3.0).abs() < 1e-9);
        let x = boolean(&a, &b, Op::Xor, s).unwrap();
        assert!((x.iter().map(|p| signed_area(p).abs()).sum::<f64>() - 6.0).abs() < 1e-9);
        assert!(boolean(&[], &[], Op::Union, s).unwrap().is_empty());
        assert_eq!(signed_area(&[Point::ZERO]), 0.0);
    }

    #[test]
    fn offsets_with_each_join() {
        let a = [sq(0.0, 0.0, 10.0, 10.0)];
        let s = DEFAULT_SCALE;
        let inner = offset(&a, -1.0, Join::Miter { limit: 2.0 }, s).unwrap();
        assert!((total(&inner).abs() - 64.0).abs() < 1e-6);
        let outer = offset(&a, 1.0, Join::Square, s).unwrap();
        assert!(total(&outer).abs() > 100.0);
        let round = offset(&a, 1.0, Join::Round { arc_tolerance: 0.01 }, s).unwrap();
        let area = total(&round).abs();
        assert!(
            (area - (144.0 - 4.0 + std::f64::consts::PI)).abs() < 0.1,
            "{area}"
        );
        // a 10 mm stroke 2 mm wide: a 10 × 2 band and two half discs
        let stroke = offset_open(&[vec![Point::ZERO, Point::new(10.0, 0.0)]], 1.0, 0.001, s).unwrap();
        let area = total(&stroke).abs();
        assert!((area - (20.0 + std::f64::consts::PI)).abs() < 0.01, "{area}");
    }
}
