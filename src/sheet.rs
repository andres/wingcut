//! Sheeting: shrink a section into the foam core that, once skinned, gives
//! the true airfoil.

use crate::airfoil::{Profile, cosine_stations};
use crate::config::Sheeting;
use crate::geom::{P2, offset_polyline_var, signed_area};
use crate::wing::enforce_te_min;
use anyhow::{Result, bail};

/// Miter limit for the skin offset (× thickness); only matters at kinks.
const SKIN_MITER: f64 = 4.0;

/// Skin thickness at `x` (mm along the chord) on one surface.
///
/// The change from the front-zone skin to the top/bottom skin starts at
/// `front_to` and is a smooth S-curve, not a sheer step: a sharp inside
/// corner can't be followed by a round hot wire (the kerf offset folds
/// there). Its length `L = max(Δ, 2.5·√Δ)` mm for a step of `Δ` mm keeps the
/// inside radius above 1.2 mm, larger than any kerf radius.
fn thickness(sh: &Sheeting, upper: bool, x: f64, chord: f64) -> f64 {
    let rear = if upper { sh.top } else { sh.bottom };
    let start = sh.front_to * chord;
    let step = (sh.front - rear).abs();
    let len = step.max(2.5 * step.sqrt());
    if x < start {
        sh.front
    } else if x < start + len {
        let u = (x - start) / len;
        sh.front + (rear - sh.front) * (1.0 - (std::f64::consts::PI * u).cos()) / 2.0
    } else {
        rear
    }
}

/// The core for a chord-frame section `p` (LE at the origin, scaled by
/// `chord`, untwisted): offset inward by the skin, resampled to the same
/// point count, with the TE thickened to `te_min`. Without skin this is
/// just the TE fix, so unsheeted output is unchanged.
pub fn core(p: &Profile, chord: f64, sh: &Sheeting, te_min: f64) -> Result<Profile> {
    let mut out = if sh.no_skin() {
        p.clone()
    } else {
        skinned(p, chord, sh)?
    };
    enforce_te_min(&mut out, te_min, chord);
    Ok(out)
}

fn skinned(p: &Profile, chord: f64, sh: &Sheeting) -> Result<Profile> {
    let n = p.n();
    let line = p.cut_order(); // upper TE → LE (index n-1) → lower TE
    let ds: Vec<f64> = line
        .iter()
        .enumerate()
        .map(|(i, q)| thickness(sh, i < n - 1, q.x, chord))
        .collect();
    // Inward = to the left of a counter-clockwise loop.
    let sign = if signed_area(&line) > 0.0 { 1.0 } else { -1.0 };
    let ds: Vec<f64> = ds.iter().map(|d| sign * d).collect();
    let off = offset_polyline_var(&line, &ds, SKIN_MITER);

    // Split at the new nose and make each surface a function of x.
    let k = (0..off.len())
        .min_by(|&a, &b| off[a].x.total_cmp(&off[b].x))
        .expect("non-empty");
    let monotone = |pts: Vec<P2>| -> Vec<P2> {
        let mut v: Vec<P2> = Vec::with_capacity(pts.len());
        for q in pts {
            if v.last().is_none_or(|l: &P2| q.x > l.x) {
                v.push(q);
            }
        }
        v
    };
    let upper = monotone(off[..=k].iter().rev().copied().collect());
    let lower = monotone(off[k..].to_vec());
    let x0 = off[k].x;
    let x1 = upper[upper.len() - 1].x.min(lower[lower.len() - 1].x);
    if upper.len() < 3 || lower.len() < 3 || x1 - x0 < 0.05 * chord {
        bail!(
            "the sheeting ({} / {} / {} mm) leaves no core on a {chord:.1} mm chord",
            sh.top,
            sh.bottom,
            sh.front
        );
    }
    let xs: Vec<f64> = cosine_stations(n)
        .iter()
        .map(|s| x0 + s * (x1 - x0))
        .collect();
    let at = |pts: &[P2], x: f64| -> P2 {
        let i = pts.partition_point(|q| q.x <= x).clamp(1, pts.len() - 1);
        let (a, b) = (pts[i - 1], pts[i]);
        let t = if b.x > a.x {
            ((x - a.x) / (b.x - a.x)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        P2::new(x, a.y + (b.y - a.y) * t)
    };
    let mut up: Vec<P2> = xs.iter().map(|&x| at(&upper, x)).collect();
    let mut lo: Vec<P2> = xs.iter().map(|&x| at(&lower, x)).collect();
    up[0] = off[k];
    lo[0] = off[k];
    // The skins must not meet in the front half (behind it, the TE fix
    // takes over where the core gets too thin).
    if let Some((u, l)) = up
        .iter()
        .zip(&lo)
        .skip(1)
        .find(|(u, l)| u.x < 0.5 * chord && u.y <= l.y)
    {
        bail!(
            "the sheeting ({} top / {} bottom / {} front mm) is thicker than the section at x = {:.1} mm (core {:.2} mm thick)",
            sh.top,
            sh.bottom,
            sh.front,
            u.x,
            u.y - l.y
        );
    }
    Ok(Profile {
        upper: up,
        lower: lo,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::airfoil::{Airfoil, tests::naca4, tests::selig_text};

    fn section(chord: f64) -> Profile {
        let (u, l) = naca4(0.0, 0.0, 0.12, 121);
        Airfoil::parse(&selig_text(&u, &l))
            .unwrap()
            .resample(401)
            .map(|q| q * chord)
    }

    fn sheet(top: f64, bottom: f64, front: f64) -> Sheeting {
        Sheeting {
            top,
            bottom,
            front,
            ..Sheeting::default()
        }
    }

    /// Distance from `q` to the polyline `line`.
    fn dist_to(line: &[P2], q: P2) -> f64 {
        line.windows(2)
            .map(|s| {
                let (a, b) = (s[0], s[1]);
                let ab = b - a;
                let t = ((q - a).dot(ab) / ab.dot(ab)).clamp(0.0, 1.0);
                q.dist(a + ab * t)
            })
            .fold(f64::MAX, f64::min)
    }

    #[test]
    fn no_skin_is_only_the_te_fix() {
        let p = section(200.0);
        let mut want = p.clone();
        enforce_te_min(&mut want, 0.8, 200.0);
        assert_eq!(core(&p, 200.0, &Sheeting::default(), 0.8).unwrap(), want);
    }

    #[test]
    fn core_is_one_skin_inside_the_airfoil_everywhere() {
        let p = section(200.0);
        let outer = p.cut_order();
        let c = core(&p, 200.0, &sheet(1.0, 0.6, 1.5), 0.0).unwrap();
        for (surface, pts, skin) in [("upper", &c.upper, 1.0), ("lower", &c.lower, 0.6)] {
            for q in pts
                .iter()
                .filter(|q| q.x > 0.35 * 200.0 && q.x < 0.8 * 200.0)
            {
                let d = dist_to(&outer, *q);
                assert!((d - skin).abs() < 0.01, "{surface} at x = {:.1}: {d}", q.x);
            }
        }
        // Front zone (and the nose) use the front skin.
        for q in c
            .upper
            .iter()
            .chain(&c.lower)
            .filter(|q| q.x > 3.0 && q.x < 0.25 * 200.0)
        {
            assert!(
                (dist_to(&outer, *q) - 1.5).abs() < 0.01,
                "front at x = {:.1}",
                q.x
            );
        }
        assert!(
            (c.upper[0].x - 1.5).abs() < 0.05,
            "nose moved back by the front skin"
        );
        assert_eq!(c.upper.len(), p.upper.len());
    }

    #[test]
    fn skin_changes_along_a_smooth_curve() {
        let sh = sheet(0.6, 0.6, 1.5);
        let (x0, len) = (0.30 * 200.0, 2.5 * 0.9f64.sqrt());
        assert_eq!(thickness(&sh, true, x0 - 0.01, 200.0), 1.5);
        assert!((thickness(&sh, true, x0 + len / 2.0, 200.0) - 1.05).abs() < 1e-12);
        assert_eq!(thickness(&sh, true, x0 + len, 200.0), 0.6);
        assert_eq!(thickness(&sh, false, 150.0, 200.0), 0.6);
        // Continuous: no jumps anywhere along the transition.
        let mut prev = thickness(&sh, true, x0 - 0.1, 200.0);
        for i in 0..400 {
            let t = thickness(&sh, true, x0 - 0.1 + i as f64 * 0.01, 200.0);
            assert!((t - prev).abs() < 0.01);
            prev = t;
        }
    }

    #[test]
    fn unsheeted_front_zone_keeps_the_nose() {
        let p = section(200.0);
        let c = core(&p, 200.0, &sheet(1.0, 1.0, 0.0), 0.0).unwrap();
        assert!(c.upper[0].x.abs() < 1e-9);
        let outer = p.cut_order();
        let q = c.upper.iter().find(|q| q.x > 20.0).unwrap();
        assert!(dist_to(&outer, *q) < 0.01);
    }

    #[test]
    fn te_keeps_the_minimum_thickness_after_skinning() {
        let p = section(200.0);
        let c = core(&p, 200.0, &sheet(1.0, 1.0, 1.0), 0.8).unwrap();
        let te = c.upper.len() - 1;
        assert!(c.upper[te].y - c.lower[te].y >= 0.8 - 1e-9);
    }

    #[test]
    fn too_much_skin_is_an_error() {
        let p = section(40.0); // 4.8 mm thick
        assert!(core(&p, 40.0, &sheet(30.0, 30.0, 30.0), 0.8).is_err());
    }
}
