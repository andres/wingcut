//! Per-panel cut planning: place the root/tip sections on the block faces,
//! compensate kerf, add lead-in/out, project to the towers, time each
//! segment (G93) and check the axis limits.

use crate::config::{Machine, Panel, Side};
use crate::geom::{P2, offset_polyline, polyline_len, signed_area};
use crate::path;
use crate::wing::WingModel;
use anyhow::{Context, Result, bail};

/// Tolerance (mm) on the axis limits, to absorb floating-point noise.
const LIMIT_TOL: f64 = 1e-9;

/// A machine move. `root`/`tip` are tower positions (x, y).
#[derive(Debug, Clone, PartialEq)]
pub enum Move {
    /// Wire-off move of the vertical axes only (G94).
    RapidY {
        root: f64,
        tip: f64,
    },
    /// Wire-off move of the horizontal axes only (G94).
    RapidX {
        root: f64,
        tip: f64,
    },
    HeatOn,
    HeatOff,
    Dwell {
        seconds: f64,
        note: &'static str,
    },
    /// Synchronised cutting move (G93), taking `minutes`.
    Cut {
        root: P2,
        tip: P2,
        minutes: f64,
    },
}

/// One block face (a plane at span position `s`).
#[derive(Debug, Clone)]
pub struct Face {
    pub s: f64,
    /// Which wing section sits on this face.
    pub is_wing_root: bool,
    /// Section in machine coordinates, before kerf (cut order TE→LE→TE).
    pub nominal: Vec<P2>,
    /// Kerf-compensated profile.
    pub kerfed: Vec<P2>,
    /// Full wire path on this face: entry, kerfed profile, exit.
    pub path: Vec<P2>,
    /// Kerf used for this face (mm).
    pub kerf: f64,
    /// Face speeds over the profile (mm/min); 0 if the profile is empty.
    pub speed_avg: f64,
    pub speed_min: f64,
    pub speed_max: f64,
    /// Length of `path` (mm).
    pub path_len: f64,
}

impl Face {
    /// `"wing root"` or `"wing tip"`.
    pub fn role(&self) -> &'static str {
        if self.is_wing_root {
            "wing root"
        } else {
            "wing tip"
        }
    }
}

#[derive(Debug, Clone)]
pub struct CutPlan {
    pub panel: String,
    /// Face nearer the root tower (lower s), then the other one.
    pub faces: [Face; 2],
    /// Projected tower paths (same length as the face paths).
    pub root_tower: Vec<P2>,
    pub tip_tower: Vec<P2>,
    pub moves: Vec<Move>,
    pub cut_minutes: f64,
    pub total_minutes: f64,
    pub max_tower_speed: f64,
    pub warnings: Vec<String>,
}

/// The wire line through two face points, evaluated at span position `s`.
///
/// With the block faces at `s_lo`/`s_hi`, the root tower is `s = 0` and the
/// tip tower is `s = tower_spacing`:
///
/// ```
/// use wingcut::{cut::project, geom::P2};
/// let (root_face, tip_face) = (P2::new(100.0, 50.0), P2::new(110.0, 60.0));
/// // Faces at 300 and 1300 on a 1600 mm machine: extrapolate 30 % each way.
/// assert_eq!(project(root_face, 300.0, tip_face, 1300.0, 0.0), P2::new(97.0, 47.0));
/// assert_eq!(project(root_face, 300.0, tip_face, 1300.0, 1600.0), P2::new(113.0, 63.0));
/// ```
pub fn project(p_lo: P2, s_lo: f64, p_hi: P2, s_hi: f64, s: f64) -> P2 {
    p_lo + (p_hi - p_lo) * ((s - s_lo) / (s_hi - s_lo))
}

/// Per-segment time (min) driven by the faster face, plus each face's speed.
fn segment_times(a: &[P2], b: &[P2], feed: f64) -> Vec<(f64, f64, f64)> {
    a.windows(2)
        .zip(b.windows(2))
        .map(|(sa, sb)| {
            let (la, lb) = (sa[0].dist(sa[1]), sb[0].dist(sb[1]));
            (la.max(lb) / feed, la, lb)
        })
        .collect()
}

pub fn plan_panel(model: &WingModel, m: &Machine, panel: &Panel) -> Result<CutPlan> {
    let b = &panel.block;
    let span = panel.to - panel.from;
    let (s_lo, s_hi) = (b.s_root, b.s_root + span);
    if s_lo < 0.0 || s_hi > m.tower_spacing {
        bail!(
            "panel {:?}: block faces at s = {s_lo}..{s_hi} must lie between the towers (0..{})",
            panel.name,
            m.tower_spacing
        );
    }
    let mut warnings = Vec::new();
    for s in model.stations_inside(panel) {
        warnings.push(format!(
            "station at span {s} is inside the panel; the wire cuts a straight (ruled) surface between the panel ends, so that station is not reproduced. Split the panel there."
        ));
    }

    // Wing sections on each face (wing coordinates).
    // Wing sections on each face: build the cut line (with cutouts) in the
    // chord frame, then twist and offset into wing coordinates.
    let (span_lo, span_hi, lo_is_root) = match panel.side {
        Side::Left => (panel.from, panel.to, true),
        Side::Right => (panel.to, panel.from, false),
    };
    let te = m.te_min_thickness;
    let (sec_lo, sec_hi) = (
        model.local_section_at(span_lo, te),
        model.local_section_at(span_hi, te),
    );
    let (pf_lo, pf_hi) = (model.planform_at(span_lo), model.planform_at(span_hi));
    let line = path::build(
        [&sec_lo, &sec_hi],
        [pf_lo.chord, pf_hi.chord],
        &panel.cutouts,
    )
    .with_context(|| format!("panel {:?}", panel.name))?;
    let [lo, hi] = line.faces;
    let lo: Vec<P2> = lo.into_iter().map(|q| pf_lo.to_wing(q)).collect();
    let hi: Vec<P2> = hi.into_iter().map(|q| pf_hi.to_wing(q)).collect();

    // Place on the machine: LE away from the operator (+x), TE towards the
    // block front at x0 + inset; the wire enters from the front.
    let all = || lo.iter().chain(hi.iter());
    let x_max = all().map(|p| p.x).fold(f64::MIN, f64::max);
    let (y_min, y_max) = all().fold((f64::MAX, f64::MIN), |(a, c), p| (a.min(p.y), c.max(p.y)));
    let y_ref = match b.chord_y {
        Some(cy) => b.y0 + cy,
        None => b.y0 + b.height / 2.0 - (y_min + y_max) / 2.0,
    };
    let x_ref = b.x0 + b.inset + x_max;
    let place = |v: &[P2]| -> Vec<P2> {
        v.iter()
            .map(|p| P2::new(x_ref - p.x, y_ref + p.y))
            .collect()
    };
    let (n_lo, n_hi) = (place(&lo), place(&hi));
    for (name, v) in [("root-side", &n_lo), ("tip-side", &n_hi)] {
        let out = v
            .iter()
            .any(|p| p.y < b.y0 || p.y > b.y0 + b.height || p.x < b.x0 || p.x > b.x0 + b.width);
        if out {
            warnings.push(format!(
                "{name} profile does not fit inside the block ({} x {} mm at x0 {}, y0 {})",
                b.width, b.height, b.x0, b.y0
            ));
        }
    }

    // Kerf from each face's average speed over the profile.
    let times = segment_times(&n_lo, &n_hi, m.feed_cut);
    let t_total: f64 = times.iter().map(|t| t.0).sum();
    let avg = |v: &[P2]| polyline_len(v) / t_total;
    let (v_lo, v_hi) = (avg(&n_lo), avg(&n_hi));
    let (k_lo, k_hi) = (m.kerf_at(v_lo), m.kerf_at(v_hi));
    let offset = |v: &[P2], kerf: f64| {
        // Outward = to the right of a counter-clockwise loop.
        let sign = if signed_area(v) > 0.0 { -1.0 } else { 1.0 };
        offset_polyline(v, sign * kerf / 2.0, m.miter_limit)
    };
    let (kf_lo, kf_hi) = (offset(&n_lo, k_lo), offset(&n_hi, k_hi));

    // Lead-in from in front of the block at TE height, lead-out the same way.
    let x_out = b.x0 - m.lead_clearance;
    let with_leads = |v: &[P2]| -> Vec<P2> {
        let mut p = Vec::with_capacity(v.len() + 2);
        p.push(P2::new(x_out, v[0].y));
        p.extend_from_slice(v);
        p.push(P2::new(x_out, v[v.len() - 1].y));
        p
    };
    let (p_lo, p_hi) = (with_leads(&kf_lo), with_leads(&kf_hi));

    // Project every point pair onto the tower planes.
    let proj = |s: f64| -> Vec<P2> {
        p_lo.iter()
            .zip(&p_hi)
            .map(|(a, c)| project(*a, s_lo, *c, s_hi, s))
            .collect()
    };
    let root_tower = proj(0.0);
    let tip_tower = proj(m.tower_spacing);
    check_limits(m, &panel.name, &root_tower, &tip_tower, line.le_index)?;

    // Moves.
    let le_idx = line.le_index + 1; // the path has the lead-in point first
    let mut moves = vec![
        Move::RapidY {
            root: root_tower[0].y,
            tip: tip_tower[0].y,
        },
        Move::RapidX {
            root: root_tower[0].x,
            tip: tip_tower[0].x,
        },
        Move::HeatOn,
    ];
    if m.preheat_s > 0.0 {
        moves.push(Move::Dwell {
            seconds: m.preheat_s,
            note: "preheat",
        });
    }
    let mut cut_minutes = 0.0;
    let mut max_tower_speed: f64 = 0.0;
    let seg = segment_times(&p_lo, &p_hi, m.feed_cut);
    let mut speeds: [Vec<f64>; 2] = [vec![], vec![]];
    for (i, &(t, la, lb)) in seg.iter().enumerate() {
        let j = i + 1;
        if la < 1e-9 && lb < 1e-9 {
            continue;
        }
        cut_minutes += t;
        let in_profile = j > 1 && j < p_lo.len() - 1; // not a lead segment
        if in_profile {
            speeds[0].push(la / t);
            speeds[1].push(lb / t);
        }
        let tw = root_tower[j - 1]
            .dist(root_tower[j])
            .max(tip_tower[j - 1].dist(tip_tower[j]));
        max_tower_speed = max_tower_speed.max(tw / t);
        moves.push(Move::Cut {
            root: root_tower[j],
            tip: tip_tower[j],
            minutes: t,
        });
        if j == le_idx && m.le_dwell_s > 0.0 {
            moves.push(Move::Dwell {
                seconds: m.le_dwell_s,
                note: "leading edge",
            });
        }
    }
    moves.push(Move::HeatOff);
    let end = root_tower.len() - 1;
    moves.push(Move::RapidX {
        root: m.park_x,
        tip: m.park_x,
    });
    let park_minutes =
        (root_tower[end].x - m.park_x).hypot(tip_tower[end].x - m.park_x) / m.feed_rapid;
    let total_minutes = cut_minutes + park_minutes + (m.preheat_s + m.le_dwell_s) / 60.0;

    let face = |s, is_wing_root, nominal: Vec<P2>, kerfed, path: Vec<P2>, kerf, sp: &[f64]| Face {
        s,
        is_wing_root,
        speed_avg: polyline_len(&nominal) / t_total,
        speed_min: sp.iter().copied().reduce(f64::min).unwrap_or(0.0),
        speed_max: sp.iter().copied().reduce(f64::max).unwrap_or(0.0),
        path_len: polyline_len(&path),
        nominal,
        kerfed,
        path,
        kerf,
    };
    Ok(CutPlan {
        panel: panel.name.clone(),
        faces: [
            face(s_lo, lo_is_root, n_lo, kf_lo, p_lo, k_lo, &speeds[0]),
            face(s_hi, !lo_is_root, n_hi, kf_hi, p_hi, k_hi, &speeds[1]),
        ],
        root_tower,
        tip_tower,
        moves,
        cut_minutes,
        total_minutes,
        max_tower_speed,
        warnings,
    })
}

/// One projected point outside the machine's travel.
#[derive(Debug, Clone, PartialEq)]
pub struct Violation {
    /// `"root"` or `"tip"` tower, or `"park"`.
    pub tower: &'static str,
    /// Configured axis letter.
    pub axis: String,
    pub value: f64,
    pub limits: [f64; 2],
    /// Index in the tower path (lead-in start = 0); `None` for the park move.
    pub index: Option<usize>,
    /// Human-readable location, e.g. "upper surface point 12".
    pub location: String,
}

/// Every axis-limit violation of a panel. Returned (inside `anyhow::Error`)
/// by [`plan_panel`]; callers such as a GUI can `downcast_ref` it to show
/// the offending points.
#[derive(Debug, Clone, PartialEq)]
pub struct LimitError {
    pub panel: String,
    pub violations: Vec<Violation>,
}

impl std::fmt::Display for LimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        const SHOWN: usize = 8;
        let n = self.violations.len();
        write!(f, "panel {:?}: {n} axis limit violation(s):", self.panel)?;
        for v in self.violations.iter().take(SHOWN) {
            let at = v
                .index
                .map(|i| format!(" (path index {i})"))
                .unwrap_or_default();
            write!(
                f,
                "\n  {} tower {} = {:.3} outside [{}, {}] at {}{at}",
                v.tower, v.axis, v.value, v.limits[0], v.limits[1], v.location
            )?;
        }
        if n > SHOWN {
            write!(f, "\n  ... and {} more", n - SHOWN)?;
        }
        Ok(())
    }
}

impl std::error::Error for LimitError {}

fn check_limits(
    m: &Machine,
    panel: &str,
    root: &[P2],
    tip: &[P2],
    le_index: usize,
) -> Result<(), LimitError> {
    let n_path = root.len();
    // `contains` is false for NaN, so non-finite values are violations too.
    let inside = |v: f64, lim: [f64; 2]| (lim[0] - LIMIT_TOL..=lim[1] + LIMIT_TOL).contains(&v);
    let location = |i: usize| -> String {
        if i == 0 {
            "lead-in start".into()
        } else if i == n_path - 1 {
            "lead-out end".into()
        } else if i - 1 <= le_index {
            format!("upper surface point {}", i - 1)
        } else {
            format!("lower surface point {}", i - 1)
        }
    };
    let ax = &m.axes;
    let mut violations = Vec::new();
    for (tower, pts, xa, ya) in [("root", root, &ax[0], &ax[1]), ("tip", tip, &ax[2], &ax[3])] {
        for (i, p) in pts.iter().enumerate() {
            for (axis, value, limits) in [(xa, p.x, m.limits.x), (ya, p.y, m.limits.y)] {
                if !inside(value, limits) {
                    violations.push(Violation {
                        tower,
                        axis: axis.clone(),
                        value,
                        limits,
                        index: Some(i),
                        location: location(i),
                    });
                }
            }
        }
    }
    if !inside(m.park_x, m.limits.x) {
        violations.push(Violation {
            tower: "park",
            axis: format!("{}/{}", ax[0], ax[2]),
            value: m.park_x,
            limits: m.limits.x,
            index: None,
            location: "park_x".into(),
        });
    }
    if violations.is_empty() {
        Ok(())
    } else {
        Err(LimitError {
            panel: panel.to_string(),
            violations,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_with_faces_at_towers_is_identity() {
        let a = P2::new(123.4, 56.7);
        let b = P2::new(150.0, 40.0);
        assert_eq!(project(a, 0.0, b, 1600.0, 0.0), a);
        assert_eq!(project(a, 0.0, b, 1600.0, 1600.0), b);
    }

    #[test]
    fn projection_extrapolates_along_the_wire() {
        let a = P2::new(100.0, 50.0);
        let b = P2::new(110.0, 60.0);
        // Faces 300..1300 on a 1600 tower spacing: 30 % beyond each face.
        let r = project(a, 300.0, b, 1300.0, 0.0);
        let t = project(a, 300.0, b, 1300.0, 1600.0);
        assert!((r.x - 97.0).abs() < 1e-12 && (r.y - 47.0).abs() < 1e-12);
        assert!((t.x - 113.0).abs() < 1e-12 && (t.y - 63.0).abs() < 1e-12);
    }
}
