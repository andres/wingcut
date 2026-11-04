//! Airfoil `.dat` parsing (Selig and Lednicer), normalisation and
//! cosine-spaced resampling with shared sync points.

use crate::geom::P2;
use anyhow::{Context, Result, bail};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatFormat {
    Selig,
    Lednicer,
}

/// A parsed airfoil, normalised to chord 1 with the LE at (0,0) and the TE
/// midpoint at (1,0). Internally a smooth parametric spline through the
/// original points (TE → upper → LE → lower → TE).
#[derive(Debug, Clone)]
pub struct Airfoil {
    pub name: String,
    pub format: DatFormat,
    /// Number of coordinate points in the file.
    pub n_points: usize,
    /// Gap between the upper and lower TE points (chord = 1).
    pub te_gap: f64,
    sx: Spline,
    sy: Spline,
    t_le: f64,
    /// Parameter of the upper-surface TE and lower-surface TE.
    t_te_upper: f64,
    t_te_lower: f64,
    xf: Transform,
}

/// A resampled profile: `upper` and `lower` both run from the LE (index 0)
/// to the TE, at identical x stations, so `upper[i]`/`lower[i]` and the same
/// index on any other profile with the same `n` are sync points.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    pub upper: Vec<P2>,
    pub lower: Vec<P2>,
}

impl Profile {
    pub fn n(&self) -> usize {
        self.upper.len()
    }

    /// Point-by-point blend: `f = 0` gives `self`, `f = 1` gives `other`.
    pub fn blend(&self, other: &Profile, f: f64) -> Profile {
        assert_eq!(self.n(), other.n(), "profiles must share a point count");
        if f == 0.0 {
            return self.clone();
        }
        if f == 1.0 {
            return other.clone();
        }
        let mix = |a: &[P2], b: &[P2]| a.iter().zip(b).map(|(p, q)| p.lerp(*q, f)).collect();
        Profile {
            upper: mix(&self.upper, &other.upper),
            lower: mix(&self.lower, &other.lower),
        }
    }

    pub fn map(&self, f: impl Fn(P2) -> P2) -> Profile {
        Profile {
            upper: self.upper.iter().map(|p| f(*p)).collect(),
            lower: self.lower.iter().map(|p| f(*p)).collect(),
        }
    }

    /// The single cut line: upper TE → LE → lower TE (2n − 1 points).
    pub fn cut_order(&self) -> Vec<P2> {
        let mut v: Vec<P2> = self.upper.iter().rev().copied().collect();
        v.extend_from_slice(&self.lower[1..]);
        v
    }
}

/// Cosine-spaced x stations in [0, 1], clustered at the LE:
/// `x = 1 - cos(pi/2 * i/(n-1))`.
///
/// Near a round LE (`y ~ sqrt(x)`) this gives roughly equal arc-length
/// segments; at the nearly straight TE it gives the coarsest spacing. A full
/// cosine distribution would also cluster at the TE, producing ~1e-4 mm
/// segments that only add noise and starve the G-code stream there.
pub fn cosine_stations(n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| {
            if i == n - 1 {
                1.0
            } else {
                1.0 - (std::f64::consts::FRAC_PI_2 * i as f64 / (n - 1) as f64).cos()
            }
        })
        .collect()
}

/// Points per surface so that no segment exceeds `max_seg` at chord `chord`.
pub fn points_for(chord: f64, max_seg: f64) -> usize {
    // Max of d/di [1 − cos(πi/(2(n−1)))] is π/(2(n−1)), at the TE.
    let n = (std::f64::consts::PI * chord / (2.0 * max_seg)).ceil() as usize + 1;
    n.max(20)
}

pub struct Stats {
    pub max_thickness: f64,
    pub max_thickness_x: f64,
    pub max_camber: f64,
    pub max_camber_x: f64,
}

impl Airfoil {
    pub fn load(path: &Path) -> Result<Airfoil> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading airfoil {}", path.display()))?;
        Airfoil::parse(&text).with_context(|| format!("parsing airfoil {}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Airfoil> {
        let mut name = String::new();
        let mut rows: Vec<(f64, f64)> = Vec::new();
        for (i, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            match parse_pair(line) {
                Some(p) => rows.push(p),
                None if rows.is_empty() && name.is_empty() => name = line.to_string(),
                None => bail!("line {}: expected two numbers, got {line:?}", i + 1),
            }
        }
        if rows.len() < 5 {
            bail!("too few coordinate points ({})", rows.len());
        }
        // Lednicer: a header row of two integer counts (upper, lower) that
        // match the rest of the file. Anything else is Selig order.
        let (c0, c1) = rows[0];
        let is_count = |v: f64| v >= 3.0 && v.fract() == 0.0;
        let lednicer = is_count(c0) && is_count(c1);
        if lednicer && c0 as usize + c1 as usize != rows.len() - 1 {
            bail!(
                "Lednicer header says {c0}+{c1} points, found {}",
                rows.len() - 1
            );
        }
        let (format, pts) = if lednicer {
            let nu = c0 as usize;
            let data = &rows[1..];
            let upper = &data[..nu];
            let lower = &data[nu..];
            let mut v: Vec<P2> = upper.iter().rev().map(|&(x, y)| P2::new(x, y)).collect();
            let skip = usize::from(P2::new(lower[0].0, lower[0].1).dist(v[v.len() - 1]) < 1e-9);
            v.extend(lower[skip..].iter().map(|&(x, y)| P2::new(x, y)));
            (DatFormat::Lednicer, v)
        } else {
            (
                DatFormat::Selig,
                rows.iter().map(|&(x, y)| P2::new(x, y)).collect(),
            )
        };
        let n_points = pts.len();
        Airfoil::from_points(name, format, n_points, pts)
    }

    fn from_points(name: String, format: DatFormat, n_points: usize, raw: Vec<P2>) -> Result<Self> {
        // Drop consecutive duplicates; they break the spline parameter.
        let mut pts: Vec<P2> = Vec::with_capacity(raw.len());
        for p in raw {
            if pts.last().is_none_or(|q: &P2| q.dist(p) > 1e-12) {
                pts.push(p);
            }
        }
        if pts.len() < 5 {
            bail!("too few distinct points");
        }
        let mut t = vec![0.0];
        for w in pts.windows(2) {
            t.push(t[t.len() - 1] + w[0].dist(w[1]));
        }
        let sx = Spline::new(&t, &pts.iter().map(|p| p.x).collect::<Vec<_>>());
        let sy = Spline::new(&t, &pts.iter().map(|p| p.y).collect::<Vec<_>>());
        let t_end = t[t.len() - 1];
        let te_mid = pts[0].lerp(pts[pts.len() - 1], 0.5);

        // LE = point on the curve farthest from the TE midpoint.
        let raw_at = |tt: f64| P2::new(sx.eval(tt), sy.eval(tt));
        let samples = 4000;
        let mut best = (0.0, -1.0);
        for i in 0..=samples {
            let tt = t_end * i as f64 / samples as f64;
            let d = raw_at(tt).dist(te_mid);
            if d > best.1 {
                best = (tt, d);
            }
        }
        let step = t_end / samples as f64;
        let t_le = golden_max(
            |tt| raw_at(tt).dist(te_mid),
            (best.0 - step).max(0.0),
            (best.0 + step).min(t_end),
        );
        let le = raw_at(t_le);
        let chord = le.dist(te_mid);
        if chord < 1e-9 || t_le <= 0.0 || t_le >= t_end {
            bail!("cannot find a leading edge");
        }
        let xf = Transform {
            origin: le,
            angle: -(te_mid.y - le.y).atan2(te_mid.x - le.x),
            scale: 1.0 / chord,
        };
        let te_gap = pts[0].dist(pts[pts.len() - 1]) / chord;
        let mut af = Airfoil {
            name,
            format,
            n_points,
            te_gap,
            sx,
            sy,
            t_le,
            t_te_upper: 0.0,
            t_te_lower: t_end,
            xf,
        };
        // If the file runs TE → lower → LE, swap so "upper" is really upper.
        if af.at(t_le / 2.0).y < af.at((t_le + t_end) / 2.0).y {
            af.t_te_upper = t_end;
            af.t_te_lower = 0.0;
        }
        Ok(af)
    }

    /// Normalised point at spline parameter `t`.
    fn at(&self, t: f64) -> P2 {
        self.xf.apply(P2::new(self.sx.eval(t), self.sy.eval(t)))
    }

    pub fn is_open_te(&self) -> bool {
        self.te_gap > 1e-5
    }

    /// Resample to `n` cosine-spaced x stations on each surface.
    pub fn resample(&self, n: usize) -> Profile {
        let xs = cosine_stations(n);
        let surface = |t_te: f64| -> Vec<P2> {
            xs.iter()
                .enumerate()
                .map(|(i, &x)| {
                    if i == 0 {
                        P2::new(0.0, 0.0)
                    } else {
                        let tt = self.solve_x(x, t_te);
                        let p = self.at(tt);
                        P2::new(x, p.y)
                    }
                })
                .collect()
        };
        Profile {
            upper: surface(self.t_te_upper),
            lower: surface(self.t_te_lower),
        }
    }

    /// Parameter between the LE and `t_te` where the curve reaches `x`.
    fn solve_x(&self, x: f64, t_te: f64) -> f64 {
        let (mut lo, mut hi) = (self.t_le, t_te);
        if self.at(hi).x <= x {
            return hi;
        }
        for _ in 0..64 {
            let mid = 0.5 * (lo + hi);
            if self.at(mid).x < x {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    }

    pub fn stats(&self) -> Stats {
        let p = self.resample(401);
        let mut s = Stats {
            max_thickness: 0.0,
            max_thickness_x: 0.0,
            max_camber: 0.0,
            max_camber_x: 0.0,
        };
        for (u, l) in p.upper.iter().zip(&p.lower) {
            let t = u.y - l.y;
            let c = (u.y + l.y) / 2.0;
            if t > s.max_thickness {
                (s.max_thickness, s.max_thickness_x) = (t, u.x);
            }
            if c.abs() > s.max_camber.abs() {
                (s.max_camber, s.max_camber_x) = (c, u.x);
            }
        }
        s
    }
}

fn parse_pair(line: &str) -> Option<(f64, f64)> {
    let mut it = line
        .split(|c: char| c.is_whitespace() || c == ',' || c == ';')
        .filter(|s| !s.is_empty());
    let a = it.next()?.parse::<f64>().ok()?;
    let b = it.next()?.parse::<f64>().ok()?;
    (a.is_finite() && b.is_finite()).then_some((a, b))
}

#[derive(Debug, Clone, Copy)]
struct Transform {
    origin: P2,
    angle: f64,
    scale: f64,
}

impl Transform {
    fn apply(&self, p: P2) -> P2 {
        (p - self.origin).rotate_about(P2::default(), self.angle) * self.scale
    }
}

fn golden_max(f: impl Fn(f64) -> f64, mut a: f64, mut b: f64) -> f64 {
    let g = (5f64.sqrt() - 1.0) / 2.0;
    for _ in 0..80 {
        let c = b - g * (b - a);
        let d = a + g * (b - a);
        if f(c) > f(d) {
            b = d;
        } else {
            a = c;
        }
    }
    0.5 * (a + b)
}

/// Natural cubic spline y(t).
#[derive(Debug, Clone)]
struct Spline {
    t: Vec<f64>,
    y: Vec<f64>,
    m: Vec<f64>,
}

impl Spline {
    fn new(t: &[f64], y: &[f64]) -> Spline {
        let n = t.len();
        let mut m = vec![0.0; n];
        if n > 2 {
            // Tridiagonal system for the second derivatives (Thomas algorithm).
            let mut c = vec![0.0; n];
            let mut d = vec![0.0; n];
            for i in 1..n - 1 {
                let h0 = t[i] - t[i - 1];
                let h1 = t[i + 1] - t[i];
                let a = h0;
                let b = 2.0 * (h0 + h1);
                let cc = h1;
                let r = 6.0 * ((y[i + 1] - y[i]) / h1 - (y[i] - y[i - 1]) / h0);
                let den = b - a * c[i - 1];
                c[i] = cc / den;
                d[i] = (r - a * d[i - 1]) / den;
            }
            for i in (1..n - 1).rev() {
                m[i] = d[i] - c[i] * m[i + 1];
            }
        }
        Spline {
            t: t.to_vec(),
            y: y.to_vec(),
            m,
        }
    }

    fn eval(&self, tt: f64) -> f64 {
        let n = self.t.len();
        let i = match self.t.partition_point(|&v| v <= tt) {
            0 => 0,
            k if k >= n => n - 2,
            k => k - 1,
        };
        let h = self.t[i + 1] - self.t[i];
        let a = (self.t[i + 1] - tt) / h;
        let b = (tt - self.t[i]) / h;
        a * self.y[i]
            + b * self.y[i + 1]
            + ((a * a * a - a) * self.m[i] + (b * b * b - b) * self.m[i + 1]) * h * h / 6.0
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// NACA 4-digit thickness/camber, used to build test fixtures.
    pub fn naca4(m: f64, p: f64, t: f64, n: usize) -> (Vec<P2>, Vec<P2>) {
        let mut up = Vec::new();
        let mut lo = Vec::new();
        for x in cosine_stations(n) {
            let yt = 5.0
                * t
                * (0.2969 * x.sqrt() - 0.1260 * x - 0.3516 * x * x + 0.2843 * x.powi(3)
                    - 0.1036 * x.powi(4));
            let (yc, dyc) = if m == 0.0 {
                (0.0, 0.0)
            } else if x < p {
                (
                    m / (p * p) * (2.0 * p * x - x * x),
                    2.0 * m / (p * p) * (p - x),
                )
            } else {
                (
                    m / ((1.0 - p) * (1.0 - p)) * (1.0 - 2.0 * p + 2.0 * p * x - x * x),
                    2.0 * m / ((1.0 - p) * (1.0 - p)) * (p - x),
                )
            };
            let th = dyc.atan();
            up.push(P2::new(x - yt * th.sin(), yc + yt * th.cos()));
            lo.push(P2::new(x + yt * th.sin(), yc - yt * th.cos()));
        }
        (up, lo)
    }

    pub fn selig_text(up: &[P2], lo: &[P2]) -> String {
        let mut s = String::from("TEST\n");
        for p in up.iter().rev().chain(lo.iter().skip(1)) {
            s += &format!("{:.6} {:.6}\n", p.x, p.y);
        }
        s
    }

    #[test]
    fn parses_selig_and_lednicer_identically() {
        let (up, lo) = naca4(0.02, 0.4, 0.12, 81);
        let selig = Airfoil::parse(&selig_text(&up, &lo)).unwrap();
        let mut led = format!("LED\n{}. {}.\n\n", up.len(), lo.len());
        assert!(led.contains("81. 81."));
        for p in &up {
            led += &format!("{:.6} {:.6}\n", p.x, p.y);
        }
        led += "\n";
        for p in &lo {
            led += &format!("{:.6} {:.6}\n", p.x, p.y);
        }
        let ledn = Airfoil::parse(&led).unwrap();
        assert_eq!(selig.format, DatFormat::Selig);
        assert_eq!(ledn.format, DatFormat::Lednicer);
        let (a, b) = (selig.resample(101), ledn.resample(101));
        for (p, q) in a.cut_order().iter().zip(b.cut_order()) {
            assert!(p.dist(q) < 1e-6);
        }
    }

    #[test]
    fn resample_matches_analytic_naca() {
        let (up, lo) = naca4(0.02, 0.4, 0.12, 161);
        let af = Airfoil::parse(&selig_text(&up, &lo)).unwrap();
        let st = af.stats();
        assert!(
            (st.max_thickness - 0.12).abs() < 0.002,
            "{}",
            st.max_thickness
        );
        assert!((st.max_camber - 0.02).abs() < 0.001);
        assert!((st.max_camber_x - 0.4).abs() < 0.03);
        let p = af.resample(201);
        assert_eq!(p.upper.len(), 201);
        assert_eq!(p.upper[0], P2::new(0.0, 0.0));
        assert!(p.upper[100].y > p.lower[100].y);
        // Same x stations on both surfaces.
        for (u, l) in p.upper.iter().zip(&p.lower) {
            assert_eq!(u.x, l.x);
        }
    }

    #[test]
    fn normalises_scaled_shifted_rotated_input() {
        let (up, lo) = naca4(0.0, 0.0, 0.12, 121);
        let a = Airfoil::parse(&selig_text(&up, &lo)).unwrap();
        let moved: (Vec<P2>, Vec<P2>) = {
            let f = |p: &P2| (p.rotate_about(P2::default(), 0.1) * 250.0) + P2::new(30.0, -7.0);
            (up.iter().map(f).collect(), lo.iter().map(f).collect())
        };
        let b = Airfoil::parse(&selig_text(&moved.0, &moved.1)).unwrap();
        let (pa, pb) = (a.resample(81), b.resample(81));
        for (p, q) in pa.cut_order().iter().zip(pb.cut_order()) {
            assert!(p.dist(q) < 1e-4, "{p:?} vs {q:?}");
        }
    }

    #[test]
    fn detects_reversed_point_order() {
        let (up, lo) = naca4(0.04, 0.4, 0.12, 81);
        let fwd = selig_text(&up, &lo);
        let mut lines: Vec<&str> = fwd.lines().skip(1).collect();
        lines.reverse();
        let rev = format!("REV\n{}\n", lines.join("\n"));
        let (a, b) = (
            Airfoil::parse(&fwd).unwrap().resample(51),
            Airfoil::parse(&rev).unwrap().resample(51),
        );
        for (p, q) in a.upper.iter().zip(&b.upper) {
            assert!(p.dist(*q) < 1e-6);
        }
    }

    #[test]
    fn detects_open_and_closed_te() {
        let (up, lo) = naca4(0.0, 0.0, 0.12, 81); // closed-TE NACA formula
        let af = Airfoil::parse(&selig_text(&up, &lo)).unwrap();
        assert!(!af.is_open_te());
        let (mut up2, mut lo2) = (up.clone(), lo.clone());
        up2.last_mut().unwrap().y = 0.0013;
        lo2.last_mut().unwrap().y = -0.0013;
        let af = Airfoil::parse(&selig_text(&up2, &lo2)).unwrap();
        assert!(af.is_open_te());
        assert!((af.te_gap - 0.0026).abs() < 1e-5);
    }

    #[test]
    fn rejects_garbage() {
        assert!(Airfoil::parse("name\n0 0\n1 1\n").is_err());
        assert!(Airfoil::parse("name\n1 0\n0.5 0.1\nhello world\n0 0\n0.5 -0.1\n1 0\n").is_err());
    }

    #[test]
    fn cosine_spacing_and_point_count() {
        let s = cosine_stations(5);
        assert_eq!(s[0], 0.0);
        assert_eq!(s[4], 1.0);
        assert!((s[2] - (1.0 - 0.5f64.sqrt())).abs() < 1e-12);
        // Densest at the LE.
        assert!(s[1] - s[0] < s[4] - s[3]);
        let n = points_for(600.0, 0.2);
        let xs = cosine_stations(n);
        let max = xs
            .windows(2)
            .map(|w| (w[1] - w[0]) * 600.0)
            .fold(0.0, f64::max);
        assert!(max <= 0.2 + 1e-9);
    }

    #[test]
    fn loads_example_files() {
        for f in ["naca2412", "naca2410"] {
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("examples/airfoils")
                .join(format!("{f}.dat"));
            let af = Airfoil::load(&path).unwrap();
            let st = af.stats();
            assert!((st.max_camber - 0.02).abs() < 0.002, "{f}");
        }
    }
}
