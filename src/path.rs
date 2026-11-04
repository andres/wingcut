//! The cut line on both faces, in each section's chord frame, with cutouts
//! (spar notches, hinge cuts) as slots.
//!
//! Both faces are built from the same list of pieces (surface run, wall,
//! floor, wall, ...), and every piece gets the same number of points on both
//! faces, so slot corners are sync points and the point counts always match.
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
    /// Index of the leading edge in each face's points.
    pub le_index: usize,
}

/// Chord fraction → cosine index space (inverse of `x = 1 − cos(π/2 · s)`).
fn s_of(f: f64) -> f64 {
    (1.0 - f.clamp(0.0, 1.0)).acos() / FRAC_PI_2
}

fn f_of(s: f64) -> f64 {
    1.0 - (FRAC_PI_2 * s).cos()
}

/// One face's section: surfaces as functions of the chord fraction.
struct Face<'a> {
    p: &'a Profile,
    chord: f64,
}

impl Face<'_> {
    /// Surface y at chord fraction `f`, interpolated between stations.
    fn y(&self, surface: Surface, f: f64) -> f64 {
        let pts = match surface {
            Surface::Upper => &self.p.upper,
            Surface::Lower => &self.p.lower,
        };
        let x = f.clamp(0.0, 1.0) * self.chord;
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

/// A slot as resolved on one face.
struct Slot {
    /// Edge fractions in cutting order (entry edge first).
    f_in: f64,
    f_out: f64,
    floor: f64,
}

fn slot_on(face: &Face, c: &Cutout, upper_first: bool) -> Slot {
    let hw = c.width / 2.0 / face.chord;
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
    // The upper surface is cut TE → LE (decreasing x), the lower LE → TE.
    let (f_in, f_out) = if upper_first {
        (c.x + hw, c.x - hw)
    } else {
        (c.x - hw, c.x + hw)
    };
    Slot { f_in, f_out, floor }
}

/// Build the cut line for two sections with `n` points per surface.
pub fn build(sections: [&Profile; 2], chords: [f64; 2], cutouts: &[Cutout]) -> Result<CutLine> {
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
    let mut out: [Vec<P2>; 2] = [Vec::new(), Vec::new()];

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
        let f_start = if upper { 1.0 } else { 0.0 };
        let f_end = 1.0 - f_start;
        let mut runs: Vec<[(f64, f64); 2]> = Vec::new(); // surface runs per face
        let mut slots: Vec<[Slot; 2]> = Vec::new();
        let mut f_face = [f_start, f_start];
        for (k, c) in cs.iter().enumerate() {
            let sl = [slot_on(&faces[0], c, upper), slot_on(&faces[1], c, upper)];
            for (fi, (face, s)) in faces.iter().zip(&sl).enumerate() {
                let label = || format!("{surface:?} cutout at x = {}", c.x).to_lowercase();
                let lo = s.f_in.min(s.f_out);
                let hi = s.f_in.max(s.f_out);
                if lo <= 0.0 || hi >= 1.0 {
                    bail!(
                        "{} is wider than the section allows ({:.1} mm chord)",
                        label(),
                        face.chord
                    );
                }
                let before = if upper {
                    s.f_in < f_face[fi]
                } else {
                    s.f_in > f_face[fi]
                };
                if k > 0 && !before {
                    bail!(
                        "{} overlaps the previous cutout on the same surface",
                        label()
                    );
                }
                // The floor must stay inside the section across the slot.
                for f in [lo, c.x, hi] {
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
                f_face[fi] = s.f_out;
            }
            let start = |fi: usize| slots.last().map_or(f_start, |s: &[Slot; 2]| s[fi].f_out);
            runs.push([(start(0), sl[0].f_in), (start(1), sl[1].f_in)]);
            slots.push(sl);
        }
        let start = |fi: usize| slots.last().map_or(f_start, |s: &[Slot; 2]| s[fi].f_out);
        runs.push([(start(0), f_end), (start(1), f_end)]);

        for (k, run) in runs.iter().enumerate() {
            push(&mut out, surface_run(&faces, surface, *run, n));
            if let Some(sl) = slots.get(k) {
                let wall_in = |fi: usize| {
                    let (face, s) = (&faces[fi], &sl[fi]);
                    vec![
                        face.pt(surface, s.f_in),
                        P2::new(s.f_in * face.chord, s.floor),
                    ]
                };
                push(&mut out, [wall_in(0), wall_in(1)]);
                let floor = |fi: usize| {
                    let (face, s) = (&faces[fi], &sl[fi]);
                    vec![
                        P2::new(s.f_in * face.chord, s.floor),
                        P2::new(s.f_out * face.chord, s.floor),
                    ]
                };
                push(&mut out, [floor(0), floor(1)]);
                let wall_out = |fi: usize| {
                    let (face, s) = (&faces[fi], &sl[fi]);
                    vec![
                        P2::new(s.f_out * face.chord, s.floor),
                        face.pt(surface, s.f_out),
                    ]
                };
                push(&mut out, [wall_out(0), wall_out(1)]);
            }
        }
        if upper {
            le_index = out[0].len() - 1;
        }
    }
    debug_assert_eq!(out[0].len(), out[1].len());
    Ok(CutLine {
        faces: out,
        le_index,
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
            width,
            depth: Some(depth),
            leave: None,
        }
    }

    #[test]
    fn without_cutouts_equals_the_profile_in_cut_order() {
        let (a, b) = sections(301);
        let cl = build([&a, &b], [200.0, 120.0], &[]).unwrap();
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
        let cl = build([&a, &b], [200.0, 120.0], &[c]).unwrap();
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
            width: 1.0,
            depth: None,
            leave: Some(0.5),
        };
        let cl = build([&a, &b], [200.0, 120.0], &[c]).unwrap();
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
                width: 1.0,
                depth: None,
                leave: Some(0.4),
            },
        ];
        let cl = build([&a, &b], [200.0, 120.0], &cuts).unwrap();
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
            build([&a, &b], [200.0, 120.0], &[c])
                .unwrap_err()
                .to_string()
        };
        assert!(err(spar(Surface::Upper, 0.3, 10.0, 40.0)).contains("goes through the section"));
        assert!(err(spar(Surface::Upper, 0.02, 10.0, 1.0)).contains("wider than the section"));
        let two = [
            spar(Surface::Upper, 0.3, 20.0, 2.0),
            spar(Surface::Upper, 0.35, 20.0, 2.0),
        ];
        let e = build([&a, &b], [200.0, 120.0], &two)
            .unwrap_err()
            .to_string();
        assert!(e.contains("overlaps"), "{e}");
    }
}
