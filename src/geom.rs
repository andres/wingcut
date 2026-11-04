//! Small 2D geometry helpers: points, polyline length and a count-preserving
//! polyline offset (used for kerf compensation).

use std::ops::{Add, Mul, Sub};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct P2 {
    pub x: f64,
    pub y: f64,
}

impl P2 {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    pub fn len(self) -> f64 {
        self.x.hypot(self.y)
    }
    pub fn dot(self, o: P2) -> f64 {
        self.x * o.x + self.y * o.y
    }
    pub fn cross(self, o: P2) -> f64 {
        self.x * o.y - self.y * o.x
    }
    pub fn dist(self, o: P2) -> f64 {
        (self - o).len()
    }
    pub fn lerp(self, o: P2, f: f64) -> P2 {
        self + (o - self) * f
    }
    /// Rotate by `angle` radians (counter-clockwise) about `c`.
    pub fn rotate_about(self, c: P2, angle: f64) -> P2 {
        let (s, co) = angle.sin_cos();
        let d = self - c;
        c + P2::new(d.x * co - d.y * s, d.x * s + d.y * co)
    }
}

impl Add for P2 {
    type Output = P2;
    fn add(self, o: P2) -> P2 {
        P2::new(self.x + o.x, self.y + o.y)
    }
}
impl Sub for P2 {
    type Output = P2;
    fn sub(self, o: P2) -> P2 {
        P2::new(self.x - o.x, self.y - o.y)
    }
}
impl Mul<f64> for P2 {
    type Output = P2;
    fn mul(self, k: f64) -> P2 {
        P2::new(self.x * k, self.y * k)
    }
}

pub fn polyline_len(pts: &[P2]) -> f64 {
    pts.windows(2).map(|w| w[0].dist(w[1])).sum()
}

/// Signed area of the polygon closed from `pts` (positive = counter-clockwise).
pub fn signed_area(pts: &[P2]) -> f64 {
    let n = pts.len();
    (0..n).map(|i| pts[i].cross(pts[(i + 1) % n])).sum::<f64>() / 2.0
}

/// Intersection of segments a0-a1 and b0-b1 (excluding exactly-collinear cases).
pub fn seg_intersect(a0: P2, a1: P2, b0: P2, b1: P2) -> Option<P2> {
    let r = a1 - a0;
    let s = b1 - b0;
    let den = r.cross(s);
    if den.abs() < 1e-15 {
        return None;
    }
    let t = (b0 - a0).cross(s) / den;
    let u = (b0 - a0).cross(r) / den;
    if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
        Some(a0 + r * t)
    } else {
        None
    }
}

/// Offset an open polyline by `d` to its left (d > 0) or right (d < 0),
/// keeping exactly one output point per input point so that sync points stay
/// paired between the root and tip faces.
///
/// * Interior vertices use a miter join, clamped to `miter_limit * |d|`.
/// * End points move along their single segment's normal.
/// * Local self-intersections (concave curvature tighter than `|d|`) are
///   collapsed onto the intersection point instead of being removed.
///
/// ```
/// use wingcut::geom::{P2, offset_polyline};
/// let line = [P2::new(0.0, 0.0), P2::new(10.0, 0.0), P2::new(10.0, 10.0)];
/// let out = offset_polyline(&line, -0.5, 4.0); // to the right
/// assert_eq!(out.len(), line.len());
/// assert!((out[1].x - 10.5).abs() < 1e-12 && (out[1].y + 0.5).abs() < 1e-12); // miter
/// ```
pub fn offset_polyline(pts: &[P2], d: f64, miter_limit: f64) -> Vec<P2> {
    let n = pts.len();
    if n < 2 || d == 0.0 {
        return pts.to_vec();
    }
    // Left normals of each segment; zero-length segments inherit a neighbour.
    let mut normals: Vec<Option<P2>> = pts
        .windows(2)
        .map(|w| {
            let t = w[1] - w[0];
            let l = t.len();
            (l > 1e-12).then(|| P2::new(-t.y / l, t.x / l))
        })
        .collect();
    let mut last = normals.iter().flatten().next().copied();
    for nm in normals.iter_mut() {
        match nm {
            Some(v) => last = Some(*v),
            None => *nm = last,
        }
    }
    let normals: Vec<P2> = normals
        .into_iter()
        .map(|v| v.unwrap_or(P2::new(0.0, 1.0)))
        .collect();

    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let p = pts[i];
        let off = if i == 0 {
            normals[0] * d
        } else if i == n - 1 {
            normals[n - 2] * d
        } else {
            let (a, b) = (normals[i - 1], normals[i]);
            let m = a + b;
            let ml = m.len();
            if ml < 1e-9 {
                // 180° reversal: fall back to the incoming normal.
                a * d
            } else {
                let m = m * (1.0 / ml);
                let cos_half = m.dot(b).max(1e-9);
                let k = (1.0 / cos_half).min(miter_limit);
                m * (d * k)
            }
        };
        out.push(p + off);
    }
    collapse_loops(&mut out, 400);
    out
}

/// Find local self-intersections (within `window` segments) and collapse the
/// points of the loop onto the intersection point. Point count is preserved.
fn collapse_loops(pts: &mut [P2], window: usize) {
    let n = pts.len();
    let mut i = 0;
    while i + 3 < n {
        let mut hit = None;
        let kmax = (i + window).min(n - 2);
        for k in (i + 2..=kmax).rev() {
            if let Some(x) = seg_intersect(pts[i], pts[i + 1], pts[k], pts[k + 1]) {
                hit = Some((k, x));
                break;
            }
        }
        if let Some((k, x)) = hit {
            for p in &mut pts[i + 1..=k] {
                *p = x;
            }
            i = k;
        } else {
            i += 1;
        }
    }
}

/// Linear interpolation in a sorted table of (x, y) pairs, clamped at the ends.
pub fn interp_table(table: &[(f64, f64)], x: f64) -> f64 {
    match table {
        [] => 0.0,
        [only] => only.1,
        _ => {
            if x <= table[0].0 {
                return table[0].1;
            }
            for w in table.windows(2) {
                let ((x0, y0), (x1, y1)) = (w[0], w[1]);
                if x <= x1 {
                    let f = if x1 > x0 { (x - x0) / (x1 - x0) } else { 1.0 };
                    return y0 + (y1 - y0) * f;
                }
            }
            table[table.len() - 1].1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn offset_straight_line_moves_left() {
        let pts = [P2::new(0.0, 0.0), P2::new(5.0, 0.0), P2::new(10.0, 0.0)];
        let o = offset_polyline(&pts, 0.5, 4.0);
        assert_eq!(o.len(), 3);
        for (p, q) in pts.iter().zip(&o) {
            assert!(close(q.x, p.x, 1e-12) && close(q.y, 0.5, 1e-12));
        }
        let o = offset_polyline(&pts, -0.5, 4.0);
        assert!(o.iter().all(|q| close(q.y, -0.5, 1e-12)));
    }

    #[test]
    fn offset_right_angle_miter() {
        // Left turn at (10,0): the outside of the corner is to the right,
        // so offsetting right gives a miter at (10.5, -0.5).
        let pts = [P2::new(0.0, 0.0), P2::new(10.0, 0.0), P2::new(10.0, 10.0)];
        let o = offset_polyline(&pts, -0.5, 4.0);
        assert!(close(o[1].x, 10.5, 1e-9) && close(o[1].y, -0.5, 1e-9));
    }

    #[test]
    fn miter_is_limited_on_sharp_spikes() {
        let pts = [P2::new(0.0, 0.0), P2::new(10.0, 0.0), P2::new(0.0, 0.1)];
        let o = offset_polyline(&pts, -1.0, 2.0);
        assert!(o[1].dist(pts[1]) <= 2.0 + 1e-9);
    }

    #[test]
    fn offset_circle_keeps_radius() {
        // Clockwise circle of radius 20, offset to the left = outward.
        let n = 721;
        let pts: Vec<P2> = (0..n)
            .map(|i| {
                let a = -(i as f64) / (n - 1) as f64 * std::f64::consts::TAU;
                P2::new(20.0 * a.cos(), 20.0 * a.sin())
            })
            .collect();
        let o = offset_polyline(&pts, -0.4, 4.0);
        // Moving to the right of a clockwise path goes towards the centre.
        for q in &o {
            assert!(close(q.len(), 19.6, 1e-3), "{}", q.len());
        }
        let o = offset_polyline(&pts, 0.4, 4.0);
        for q in &o {
            assert!(close(q.len(), 20.4, 1e-3));
        }
    }

    #[test]
    fn tight_concave_notch_collapses_without_losing_points() {
        // A narrow U-notch 0.4 wide, offset inward-of-notch by 0.5: the two
        // walls cross and must be collapsed, not left as a loop.
        let pts = [
            P2::new(0.0, 0.0),
            P2::new(5.0, 0.0),
            P2::new(5.0, -5.0),
            P2::new(5.4, -5.0),
            P2::new(5.4, 0.0),
            P2::new(10.0, 0.0),
        ];
        let o = offset_polyline(&pts, 0.5, 4.0);
        assert_eq!(o.len(), pts.len());
        for i in 0..o.len() - 1 {
            for k in i + 2..o.len() - 1 {
                if let Some(x) = seg_intersect(o[i], o[i + 1], o[k], o[k + 1]) {
                    // Only touching at a collapsed (shared) point is allowed.
                    assert!(
                        x.dist(o[i + 1]) < 1e-9 || x.dist(o[k]) < 1e-9,
                        "loop left between {i} and {k}"
                    );
                }
            }
        }
    }

    #[test]
    fn table_interpolation_clamps() {
        let t = [(120.0, 1.1), (180.0, 0.8), (240.0, 0.6)];
        assert!(close(interp_table(&t, 100.0), 1.1, 1e-12));
        assert!(close(interp_table(&t, 150.0), 0.95, 1e-12));
        assert!(close(interp_table(&t, 300.0), 0.6, 1e-12));
    }

    #[test]
    fn signed_area_orientation() {
        let ccw = [P2::new(0.0, 0.0), P2::new(1.0, 0.0), P2::new(1.0, 1.0)];
        assert!(signed_area(&ccw) > 0.0);
    }
}
