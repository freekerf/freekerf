//! Points and directions along a path by distance: where the letters of a text on
//! a path stand. The path is followed as a fine polyline; past the ends of an open
//! path the first and last directions go on, a closed path wraps around.

use freekerf_geometry::{Path, Point};

/// Tolerance of the polyline that stands for the path, mm.
const TOLERANCE_MM: f64 = 0.01;

/// A path measured by length.
#[derive(Debug, Clone)]
pub(crate) struct Along {
    pts: Vec<Point>,
    /// Length from the start to each point.
    cum: Vec<f64>,
    closed: bool,
}

impl Along {
    /// `None` for a path without length.
    pub(crate) fn new(path: &Path) -> Option<Self> {
        let mut pts: Vec<Point> = Vec::new();
        for p in path.to_polyline(TOLERANCE_MM) {
            if pts.last().is_none_or(|q| q.distance(p) > 1e-9) {
                pts.push(p);
            }
        }
        let mut cum = Vec::with_capacity(pts.len());
        let mut total = 0.0;
        for (i, p) in pts.iter().enumerate() {
            if i > 0 {
                total += pts[i - 1].distance(*p);
            }
            cum.push(total);
        }
        (pts.len() > 1 && total > 1e-9).then(|| Self {
            closed: path.closed
                || pts
                    .first()
                    .zip(pts.last())
                    .is_some_and(|(a, b)| a.distance(*b) < 1e-6),
            pts,
            cum,
        })
    }

    pub(crate) fn length(&self) -> f64 {
        self.cum.last().copied().unwrap_or(0.0)
    }

    /// The point at distance `s` from the start.
    pub(crate) fn point(&self, s: f64) -> Point {
        let len = self.length();
        let n = self.pts.len();
        let s = if self.closed { s.rem_euclid(len) } else { s };
        if s <= 0.0 {
            let d = (self.pts[1] - self.pts[0]) * (1.0 / self.cum[1]);
            return self.pts[0] + d * s;
        }
        if s >= len {
            let d = (self.pts[n - 1] - self.pts[n - 2]) * (1.0 / (self.cum[n - 1] - self.cum[n - 2]));
            return self.pts[n - 1] + d * (s - len);
        }
        let i = self.cum.partition_point(|c| *c <= s).clamp(1, n - 1);
        let (a, b) = (self.cum[i - 1], self.cum[i]);
        let t = if b > a { (s - a) / (b - a) } else { 0.0 };
        self.pts[i - 1] + (self.pts[i] - self.pts[i - 1]) * t
    }

    /// The direction at `s`, radians: of the chord from `s - half` to `s + half`
    /// (a letter `2 × half` wide sits along it).
    pub(crate) fn angle(&self, s: f64, half: f64) -> f64 {
        let half = half.max(TOLERANCE_MM * 10.0);
        let d = self.point(s + half) - self.point(s - half);
        d.y.atan2(d.x)
    }
}

#[cfg(test)]
mod tests {
    use freekerf_geometry::Segment;

    use super::*;

    #[test]
    fn lines_and_their_extensions() {
        let a = Along::new(&Path::polyline(
            &[Point::ZERO, Point::new(10.0, 0.0), Point::new(10.0, 10.0)],
            false,
        ))
        .unwrap();
        assert!((a.length() - 20.0).abs() < 1e-9);
        assert!(a.point(5.0).distance(Point::new(5.0, 0.0)) < 1e-9);
        assert!(a.point(15.0).distance(Point::new(10.0, 5.0)) < 1e-9);
        assert!(a.point(-2.0).distance(Point::new(-2.0, 0.0)) < 1e-9);
        assert!(a.point(22.0).distance(Point::new(10.0, 12.0)) < 1e-9);
        assert!(a.angle(2.0, 1.0).abs() < 1e-9);
        assert!((a.angle(15.0, 1.0) - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
        assert!(Along::new(&Path::new(Point::ZERO)).is_none());
    }

    #[test]
    fn closed_paths_wrap() {
        let mut circle = Path::new(Point::new(1.0, 0.0));
        circle
            .segments
            .push(Segment::arc(Point::new(-1.0, 0.0), Point::ZERO, true));
        circle
            .segments
            .push(Segment::arc(Point::new(1.0, 0.0), Point::ZERO, true));
        circle.closed = true;
        let a = Along::new(&circle).unwrap();
        let len = a.length();
        // the polyline is inside the circle, within the tolerance
        assert!((len - 2.0 * std::f64::consts::PI).abs() < 0.03, "{len}");
        assert!(a.point(len + 0.5).distance(a.point(0.5)) < 1e-9);
        assert!(a.point(-0.5).distance(a.point(len - 0.5)) < 1e-9);
    }
}
