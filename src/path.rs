//! The cut line on both faces, in each section's chord frame, with cutouts
//! (spar notches, hinge slots, V-grooves) and optional flat LE/TE ends.
//!
//! Both faces are built from the same list of pieces (surface run, wall,
//! floor, wall, ...), and every piece gets the same number of points on both
//! faces, so corners are sync points and the point counts always match.
//! Surface runs are sampled in the LE-clustered cosine index space, so with
//! no cutouts the result is exactly the resampled profile in cut order.

use crate::airfoil::Profile;
use crate::config::{Cutout, Surface};
use crate::geom::P2;
use anyhow::{Result, bail, ensure};
use std::f64::consts::FRAC_PI_2;

/// The cut line on both faces (chord frame, mm), upper TE → LE → lower TE.
#[derive(Debug, Clone, PartialEq)]
pub struct CutLine {
    pub faces: [Vec<P2>; 2],
    /// Index of the leading edge (or the top of the LE cap) in each face.
    pub le_index: usize,
    /// Indices of V-groove apexes (sharp inner corners).
    pub apexes: Vec<usize>,
}

/// Where the core ends: chord fractions of the flat LE cap / TE stock cuts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ends {
    /// 0 = no LE cap.
    pub le_cap: f64,
    /// 1 = no TE stock.
    pub te_stock: f64,
}

impl Default for Ends {
    fn default() -> Self {
        Ends {
            le_cap: 0.0,
            te_stock: 1.0,
        }
    }
}

/// Chord fraction → cosine index space (inverse of `x = 1 − cos(π/2 · s)`).
fn s_of(f: f64) -> f64 {
    (1.0 - f.clamp(0.0, 1.0)).acos() / FRAC_PI_2
}

fn f_of(s: f64) -> f64 {
    1.0 - (FRAC_PI_2 * s).cos()
}

/// One face's section: surfaces as functions of the (true) chord fraction.
/// The section may start behind x = 0 (a sheeted core's nose).
struct Face<'a> {
    p: &'a Profile,
    chord: f64,
}

impl Face<'_> {
    fn pts(&self, surface: Surface) -> &[P2] {
        match surface {
            Surface::Upper => &self.p.upper,
            Surface::Lower => &self.p.lower,
        }
    }

    /// Chord fractions where the section starts (nose) and ends (TE).
    fn span(&self) -> (f64, f64) {
        let (u, l) = (&self.p.upper, &self.p.lower);
        let end = u[u.len() - 1].x.min(l[l.len() - 1].x);
        (u[0].x / self.chord, end / self.chord)
    }

    /// Surface y at chord fraction `f`, interpolated between stations.
    fn y(&self, surface: Surface, f: f64) -> f64 {
        let pts = self.pts(surface);
        let x = (f * self.chord).clamp(pts[0].x, pts[pts.len() - 1].x);
        let i = pts.partition_point(|q| q.x <= x).clamp(1, pts.len() - 1);
        let (a, b) = (pts[i - 1], pts[i]);
        if b.x > a.x {
            a.y + (b.y - a.y) * (x - a.x) / (b.x - a.x)
        } else {
            a.y
        }
    }

    fn pt(&self, surface: Surface, f: f64) -> P2 {
        P2::new(f * self.chord, self.y(surface, f))
    }
}

/// A cutout as resolved on one face.
struct Slot {
    /// Edge fractions in cutting order (entry edge first).
    f_in: f64,
    f_out: f64,
    /// Floor (slot) or apex (V) height.
    floor: f64,
    /// V-groove: apex at the cutout's x instead of a flat floor.
    v: bool,
}

fn slot_on(face: &Face, c: &Cutout, upper_first: bool) -> Slot {
    let (s, other) = match c.surface {
        Surface::Upper => (Surface::Upper, Surface::Lower),
        Surface::Lower => (Surface::Lower, Surface::Upper),
    };
    let sign = if s == Surface::Upper { -1.0 } else { 1.0 };
    let floor = match (c.depth, c.leave) {
        (Some(d), _) => face.y(s, c.x) + sign * d,
        (None, Some(l)) => face.y(other, c.x) - sign * l,
        (None, None) => face.y(s, c.x),
    };
    let hw = match (c.width, c.angle) {
        (Some(w), _) => w / 2.0,
        // V: the opening width follows from the depth at the apex.
        (None, Some(a)) => (face.y(s, c.x) - floor).abs() * (a.to_radians() / 2.0).tan(),
        (None, None) => 0.0,
    } / face.chord;
    // The upper surface is cut TE → LE (decreasing x), the lower LE → TE.
    let (f_in, f_out) = if upper_first {
        (c.x + hw, c.x - hw)
    } else {
        (c.x - hw, c.x + hw)
    };
    Slot {
        f_in,
        f_out,
        floor,
        v: c.angle.is_some(),
    }
}

/// Build the cut line for two sections (same point count) with cutouts and
/// optional flat LE/TE ends.
pub fn build(
    sections: [&Profile; 2],
    chords: [f64; 2],
    cutouts: &[Cutout],
    ends: Ends,
) -> Result<CutLine> {
    let faces = [
        Face {
            p: sections[0],
            chord: chords[0],
        },
        Face {
            p: sections[1],
            chord: chords[1],
        },
    ];
    let n = sections[0].n();
    ensure!(sections[1].n() == n, "sections must share a point count");
    // Per face: where the core starts and ends along the chord.
    let mut front = [0.0; 2];
    let mut back = [1.0; 2];
    for (fi, face) in faces.iter().enumerate() {
        let (nose, te) = face.span();
        front[fi] = ends.le_cap.max(nose);
        back[fi] = ends.te_stock.min(te);
        if front[fi] >= back[fi] {
            bail!("the core is empty: LE cap / sheeting reach past the TE stock");
        }
    }
    let mut out: [Vec<P2>; 2] = [Vec::new(), Vec::new()];
    let mut apexes = Vec::new();

    // TE stock: start in the middle of the flat rear face, go up it.
    if ends.te_stock < 1.0 {
        let flat = |fi: usize| {
            let (face, f) = (&faces[fi], back[fi]);
            let mid = (face.y(Surface::Upper, f) + face.y(Surface::Lower, f)) / 2.0;
            vec![P2::new(f * face.chord, mid), face.pt(Surface::Upper, f)]
        };
        push(&mut out, [flat(0), flat(1)]);
    }

    let mut le_index = 0;
    for surface in [Surface::Upper, Surface::Lower] {
        let upper = surface == Surface::Upper;
        let mut cs: Vec<&Cutout> = cutouts.iter().filter(|c| c.surface == surface).collect();
        // Cutting order: upper by decreasing x, lower by increasing x.
        cs.sort_by(|a, b| {
            if upper {
                b.x.total_cmp(&a.x)
            } else {
                a.x.total_cmp(&b.x)
            }
        });
        let f_start = |fi: usize| if upper { back[fi] } else { front[fi] };
        let f_end = |fi: usize| if upper { front[fi] } else { back[fi] };
        let mut runs: Vec<[(f64, f64); 2]> = Vec::new(); // surface runs per face
        let mut slots: Vec<[Slot; 2]> = Vec::new();
        for c in &cs {
            let sl = [slot_on(&faces[0], c, upper), slot_on(&faces[1], c, upper)];
            for (fi, (face, s)) in faces.iter().zip(&sl).enumerate() {
                let label = || format!("{surface:?} cutout at x = {}", c.x).to_lowercase();
                let lo = s.f_in.min(s.f_out);
                let hi = s.f_in.max(s.f_out);
                if lo <= front[fi] || hi >= back[fi] {
                    bail!(
                        "{} does not fit on the core (it spans {:.3}..{:.3} of the chord; the core runs {:.3}..{:.3})",
                        label(),
                        lo,
                        hi,
                        front[fi],
                        back[fi]
                    );
                }
                let prev = slots
                    .last()
                    .map_or(f_start(fi), |p: &[Slot; 2]| p[fi].f_out);
                let after_prev = if upper { s.f_in < prev } else { s.f_in > prev };
                if !after_prev {
                    bail!(
                        "{} overlaps the previous cutout on the same surface",
                        label()
                    );
                }
                // The floor / apex must stay inside the section.
                let checks: &[f64] = if s.v { &[c.x] } else { &[lo, c.x, hi] };
                for &f in checks {
                    let (yu, yl) = (face.y(Surface::Upper, f), face.y(Surface::Lower, f));
                    if !(s.floor < yu && s.floor > yl) {
                        bail!(
                            "{} goes through the section (floor y = {:.2} mm, section {:.2}..{:.2} mm); reduce depth or increase leave",
                            label(),
                            s.floor,
                            yl,
                            yu
                        );
                    }
                }
            }
            let start = |fi: usize| {
                slots
                    .last()
                    .map_or(f_start(fi), |s: &[Slot; 2]| s[fi].f_out)
            };
            runs.push([(start(0), sl[0].f_in), (start(1), sl[1].f_in)]);
            slots.push(sl);
        }
        let start = |fi: usize| {
            slots
                .last()
                .map_or(f_start(fi), |s: &[Slot; 2]| s[fi].f_out)
        };
        runs.push([(start(0), f_end(0)), (start(1), f_end(1))]);

        for (k, run) in runs.iter().enumerate() {
            push(&mut out, surface_run(&faces, surface, *run, n));
            let Some(sl) = slots.get(k) else { continue };
            let bottom = |fi: usize, f: f64| P2::new(f * faces[fi].chord, sl[fi].floor);
            let apex_f = cs[k].x;
            if sl[0].v {
                let wall_in =
                    |fi: usize| vec![faces[fi].pt(surface, sl[fi].f_in), bottom(fi, apex_f)];
                push(&mut out, [wall_in(0), wall_in(1)]);
                apexes.push(out[0].len() - 1);
                let wall_out =
                    |fi: usize| vec![bottom(fi, apex_f), faces[fi].pt(surface, sl[fi].f_out)];
                push(&mut out, [wall_out(0), wall_out(1)]);
            } else {
                let wall_in =
                    |fi: usize| vec![faces[fi].pt(surface, sl[fi].f_in), bottom(fi, sl[fi].f_in)];
                push(&mut out, [wall_in(0), wall_in(1)]);
                let floor = |fi: usize| vec![bottom(fi, sl[fi].f_in), bottom(fi, sl[fi].f_out)];
                push(&mut out, [floor(0), floor(1)]);
                let wall_out = |fi: usize| {
                    vec![
                        bottom(fi, sl[fi].f_out),
                        faces[fi].pt(surface, sl[fi].f_out),
                    ]
                };
                push(&mut out, [wall_out(0), wall_out(1)]);
            }
        }
        if upper {
            le_index = out[0].len() - 1;
            // LE cap: straight down the flat front face.
            if ends.le_cap > 0.0 {
                let flat = |fi: usize| {
                    let (face, f) = (&faces[fi], front[fi]);
                    vec![face.pt(Surface::Upper, f), face.pt(Surface::Lower, f)]
                };
                push(&mut out, [flat(0), flat(1)]);
            }
        }
    }

    // TE stock: back up the lower half of the flat rear face.
    if ends.te_stock < 1.0 {
        let flat = |fi: usize| {
            let (face, f) = (&faces[fi], back[fi]);
            let mid = (face.y(Surface::Upper, f) + face.y(Surface::Lower, f)) / 2.0;
            vec![face.pt(Surface::Lower, f), P2::new(f * face.chord, mid)]
        };
        push(&mut out, [flat(0), flat(1)]);
    }
    debug_assert_eq!(out[0].len(), out[1].len());
    Ok(CutLine {
        faces: out,
        le_index,
        apexes,
    })
}

/// Append a piece to both faces, dropping its first point (it equals the
/// previous piece's last point) except for the very first piece.
fn push(out: &mut [Vec<P2>; 2], pieces: [Vec<P2>; 2]) {
    for (o, p) in out.iter_mut().zip(pieces) {
        let skip = usize::from(!o.is_empty());
        o.extend(p.into_iter().skip(skip));
    }
}

/// Sample a surface run on both faces with a shared point count, evenly in
/// cosine index space (so the LE stays dense and the TE coarse).
fn surface_run(
    faces: &[Face; 2],
    surface: Surface,
    run: [(f64, f64); 2],
    n: usize,
) -> [Vec<P2>; 2] {
    let steps = run
        .iter()
        .map(|&(a, b)| ((s_of(b) - s_of(a)).abs() * (n - 1) as f64 - 1e-9).ceil() as usize)
        .max()
        .unwrap_or(1)
        .max(1);
    let sample = |fi: usize| -> Vec<P2> {
        let (fa, fb) = run[fi];
        let (sa, sb) = (s_of(fa), s_of(fb));
        (0..=steps)
            .map(|j| {
                let f = match j {
                    0 => fa,
                    j if j == steps => fb,
                    j => f_of(sa + (sb - sa) * j as f64 / steps as f64),
                };
                faces[fi].pt(surface, f)
            })
            .collect()
    };
    [sample(0), sample(1)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::airfoil::{Airfoil, tests::naca4, tests::selig_text};

    fn sections(n: usize) -> (Profile, Profile) {
        let (u, l) = naca4(0.02, 0.4, 0.12, 121);
        let af = Airfoil::parse(&selig_text(&u, &l)).unwrap().resample(n);
        (af.map(|q| q * 200.0), af.map(|q| q * 120.0))
    }

    fn spar(surface: Surface, x: f64, width: f64, depth: f64) -> Cutout {
        Cutout {
            surface,
            x,
            width: Some(width),
            angle: None,
            depth: Some(depth),
            leave: None,
        }
    }

    #[test]
    fn without_cutouts_equals_the_profile_in_cut_order() {
        let (a, b) = sections(301);
        let cl = build([&a, &b], [200.0, 120.0], &[], Ends::default()).unwrap();
        for (face, p) in cl.faces.iter().zip([&a, &b]) {
            let want = p.cut_order();
            assert_eq!(face.len(), want.len());
            for (q, w) in face.iter().zip(&want) {
                assert!(q.dist(*w) < 1e-9, "{q:?} vs {w:?}");
            }
        }
        assert_eq!(cl.le_index, 300);
        assert_eq!(cl.faces[0][300], P2::new(0.0, 0.0));
    }

    #[test]
    fn spar_notch_has_exact_size_and_synced_corners() {
        let (a, b) = sections(301);
        let c = spar(Surface::Upper, 0.3, 10.0, 5.0);
        let cl = build([&a, &b], [200.0, 120.0], &[c], Ends::default()).unwrap();
        assert_eq!(cl.faces[0].len(), cl.faces[1].len());
        for (fi, chord) in [200.0, 120.0].into_iter().enumerate() {
            let f = &cl.faces[fi];
            // Exactly two points share each wall's x: find the floor.
            let floor: Vec<usize> = (1..f.len())
                .filter(|&i| {
                    (f[i].y - f[i - 1].y).abs() < 1e-12
                        && (f[i].x - f[i - 1].x - -10.0).abs() < 1e-9
                })
                .collect();
            assert_eq!(floor.len(), 1, "face {fi}");
            let i = floor[0];
            let (left, right) = (f[i], f[i - 1]);
            assert!((left.x - (0.3 * chord - 5.0)).abs() < 1e-9);
            assert!((right.x - (0.3 * chord + 5.0)).abs() < 1e-9);
            // Walls are vertical and the floor is `depth` below the surface
            // at the slot centre.
            assert_eq!(f[i - 2].x, right.x);
            assert_eq!(f[i + 1].x, left.x);
            let face = Face {
                p: if fi == 0 { &a } else { &b },
                chord,
            };
            assert!((face.y(Surface::Upper, 0.3) - 5.0 - left.y).abs() < 1e-9);
        }
        // The LE is at the same index on both faces.
        for f in &cl.faces {
            assert_eq!(f[cl.le_index], P2::new(0.0, 0.0));
        }
    }

    #[test]
    fn hinge_cut_leaves_the_requested_skin() {
        let (a, b) = sections(201);
        let c = Cutout {
            surface: Surface::Lower,
            x: 0.75,
            width: Some(1.0),
            angle: None,
            depth: None,
            leave: Some(0.5),
        };
        let cl = build([&a, &b], [200.0, 120.0], &[c], Ends::default()).unwrap();
        for (fi, chord) in [200.0, 120.0].into_iter().enumerate() {
            let face = Face {
                p: if fi == 0 { &a } else { &b },
                chord,
            };
            let floor_y = face.y(Surface::Upper, 0.75) - 0.5;
            assert!(
                cl.faces[fi]
                    .iter()
                    .any(|q| (q.y - floor_y).abs() < 1e-12
                        && (q.x - 0.75 * chord - 0.5).abs() < 1e-9)
            );
        }
    }

    #[test]
    fn several_cutouts_on_both_surfaces_stay_in_sync() {
        let (a, b) = sections(201);
        let cuts = [
            spar(Surface::Upper, 0.25, 8.0, 3.0),
            spar(Surface::Lower, 0.25, 8.0, 3.0),
            spar(Surface::Upper, 0.6, 5.0, 2.0),
            Cutout {
                surface: Surface::Lower,
                x: 0.8,
                width: Some(1.0),
                angle: None,
                depth: None,
                leave: Some(0.4),
            },
        ];
        let cl = build([&a, &b], [200.0, 120.0], &cuts, Ends::default()).unwrap();
        assert_eq!(cl.faces[0].len(), cl.faces[1].len());
        // Monotone along each run is not required, but no point may be NaN
        // and the LE must still be at the origin.
        assert!(
            cl.faces
                .iter()
                .flatten()
                .all(|q| q.x.is_finite() && q.y.is_finite())
        );
        assert_eq!(cl.faces[0][cl.le_index], P2::new(0.0, 0.0));
    }

    #[test]
    fn rejects_impossible_cutouts() {
        let (a, b) = sections(201);
        let err = |c: Cutout| {
            build([&a, &b], [200.0, 120.0], &[c], Ends::default())
                .unwrap_err()
                .to_string()
        };
        assert!(err(spar(Surface::Upper, 0.3, 10.0, 40.0)).contains("goes through the section"));
        assert!(err(spar(Surface::Upper, 0.02, 10.0, 1.0)).contains("does not fit on the core"));
        let two = [
            spar(Surface::Upper, 0.3, 20.0, 2.0),
            spar(Surface::Upper, 0.35, 20.0, 2.0),
        ];
        let e = build([&a, &b], [200.0, 120.0], &two, Ends::default())
            .unwrap_err()
            .to_string();
        assert!(e.contains("overlaps"), "{e}");
    }

    #[test]
    fn le_cap_cuts_the_nose_flat() {
        let (a, b) = sections(201);
        let ends = Ends {
            le_cap: 0.05,
            te_stock: 1.0,
        };
        let cl = build([&a, &b], [200.0, 120.0], &[], ends).unwrap();
        for (f, chord) in cl.faces.iter().zip([200.0, 120.0]) {
            let top = f[cl.le_index];
            let bottom = f[cl.le_index + 1];
            assert!(
                (top.x - 0.05 * chord).abs() < 1e-12 && top.x == bottom.x,
                "vertical flat"
            );
            assert!(top.y > bottom.y);
            assert!(
                f.iter().all(|q| q.x >= 0.05 * chord - 1e-9),
                "nothing in front of the cap"
            );
        }
    }

    #[test]
    fn te_stock_cuts_the_rear_flat_from_the_middle() {
        let (a, b) = sections(201);
        let ends = Ends {
            le_cap: 0.0,
            te_stock: 0.85,
        };
        let cl = build([&a, &b], [200.0, 120.0], &[], ends).unwrap();
        for (f, chord) in cl.faces.iter().zip([200.0, 120.0]) {
            let (first, last) = (f[0], f[f.len() - 1]);
            assert_eq!(first, last, "starts and ends in the middle of the flat");
            assert!((first.x - 0.85 * chord).abs() < 1e-12);
            assert_eq!(f[1].x, first.x); // up the flat
            assert!(f[1].y > first.y);
            assert_eq!(f[f.len() - 2].x, first.x); // and up again from below
            assert!(f.iter().all(|q| q.x <= 0.85 * chord + 1e-9));
        }
    }

    #[test]
    fn v_groove_opens_by_the_angle_and_keeps_the_skin() {
        let (a, b) = sections(201);
        let c = Cutout {
            surface: Surface::Lower,
            x: 0.75,
            width: None,
            angle: Some(30.0),
            depth: None,
            leave: Some(0.5),
        };
        let cl = build([&a, &b], [200.0, 120.0], &[c], Ends::default()).unwrap();
        assert_eq!(cl.apexes.len(), 1);
        let j = cl.apexes[0];
        for (fi, chord) in [200.0, 120.0].into_iter().enumerate() {
            let face = Face {
                p: if fi == 0 { &a } else { &b },
                chord,
            };
            let f = &cl.faces[fi];
            let (inn, apex, out) = (f[j - 1], f[j], f[j + 1]);
            assert!((apex.x - 0.75 * chord).abs() < 1e-9);
            assert!(
                (apex.y - (face.y(Surface::Upper, 0.75) - 0.5)).abs() < 1e-9,
                "hinge skin"
            );
            let h = apex.y - face.y(Surface::Lower, 0.75);
            let half = (15f64).to_radians().tan() * h;
            assert!(((out.x - inn.x) - 2.0 * half).abs() < 1e-9, "opening width");
        }
    }

    #[test]
    fn cutouts_must_sit_on_the_core() {
        let (a, b) = sections(201);
        let ends = Ends {
            le_cap: 0.0,
            te_stock: 0.7,
        };
        let c = spar(Surface::Upper, 0.75, 5.0, 1.0);
        let e = build([&a, &b], [200.0, 120.0], &[c], ends)
            .unwrap_err()
            .to_string();
        assert!(e.contains("does not fit on the core"), "{e}");
    }
}
