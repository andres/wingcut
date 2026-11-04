//! End-to-end tests for the cut pipeline, including the ones SPEC §8 requires.

use std::path::{Path, PathBuf};
use wingcut::config::{Machine, Wing};
use wingcut::cut::{CutPlan, Move, plan_panel};
use wingcut::geom::P2;
use wingcut::wing::WingModel;

fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples")
}

fn machine(extra: &str) -> Machine {
    // `extra` lines override the defaults below (TOML forbids duplicate keys,
    // so the defaults are only added when not overridden).
    let defaults = [
        ("tower_spacing", "1600"),
        ("limits", "{ x = [0, 1000], y = [0, 300] }"),
        ("feed_cut", "180"),
        (
            "kerf",
            "[ { feed = 120, kerf = 1.1 }, { feed = 180, kerf = 0.8 }, { feed = 240, kerf = 0.6 } ]",
        ),
        ("max_segment", "0.5"),
    ];
    let mut t = String::from(extra);
    t.push('\n');
    for (k, v) in defaults {
        if !extra.lines().any(|l| l.trim_start().starts_with(k)) {
            t += &format!("{k} = {v}\n");
        }
    }
    Machine::parse(&t).unwrap()
}

fn model(wing_toml: &str, m: &Machine) -> WingModel {
    let w = Wing::parse(wing_toml, &examples()).unwrap();
    WingModel::new(w, m.max_segment).unwrap()
}

fn plan(model: &WingModel, m: &Machine, name: &str) -> CutPlan {
    plan_panel(model, m, model.wing.panel(name).unwrap()).unwrap()
}

const RECT: &str = r#"
name = "rect"
[[station]]
span = 0
chord = 200
airfoil = "airfoils/naca2412.dat"
[[station]]
span = 1600
chord = 200
airfoil = "airfoils/naca2412.dat"

[[panel]]
name = "mid"
from = 300
to = 1300
block = { s_root = 300, x0 = 100, y0 = 20, height = 60, width = 300 }

[[panel]]
name = "full"
from = 0
to = 1600
block = { s_root = 0, x0 = 100, y0 = 20, height = 60, width = 300 }
"#;

const TAPER: &str = r#"
name = "taper"
[[station]]
span = 0
chord = 260
airfoil = "airfoils/naca2412.dat"
[[station]]
span = 1600
chord = 140
airfoil = "airfoils/naca2410.dat"
le_offset = 60
dihedral_offset = 20
twist_deg = -2

[[panel]]
name = "full"
from = 0
to = 1600
block = { s_root = 0, x0 = 100, y0 = 20, height = 80, width = 300 }

[[panel]]
name = "mid"
from = 200
to = 1200
block = { s_root = 300, x0 = 100, y0 = 20, height = 80, width = 300 }
"#;

fn assert_paths_eq(a: &[P2], b: &[P2], tol: f64) {
    assert_eq!(a.len(), b.len());
    for (i, (p, q)) in a.iter().zip(b).enumerate() {
        assert!(p.dist(*q) <= tol, "point {i}: {p:?} vs {q:?}");
    }
}

#[test]
fn projection_with_block_faces_at_the_towers_is_identity() {
    let m = machine("");
    let w = model(TAPER, &m);
    let p = plan(&w, &m, "full");
    assert_paths_eq(&p.root_tower, &p.faces[0].path, 1e-9);
    assert_paths_eq(&p.tip_tower, &p.faces[1].path, 1e-9);
}

#[test]
fn rectangular_untapered_panel_gives_identical_tower_paths() {
    let m = machine("");
    let w = model(RECT, &m);
    for name in ["mid", "full"] {
        let p = plan(&w, &m, name);
        assert_paths_eq(&p.root_tower, &p.tip_tower, 1e-9);
        assert_eq!(p.faces[0].kerf, p.faces[1].kerf);
    }
}

/// Parse "G1 X.. Y.. Z.. A.. F.." words into (axis letter -> value).
fn words(line: &str) -> Vec<(char, f64)> {
    line.split(';')
        .next()
        .unwrap()
        .split_whitespace()
        .filter_map(|w| {
            let c = w.chars().next()?;
            Some((c, w[1..].parse().ok()?))
        })
        .collect()
}

#[test]
fn g93_segment_times_are_equal_for_both_towers() {
    let m = machine("");
    let w = model(TAPER, &m);
    let panel = w.wing.panel("mid").unwrap();
    let p = plan_panel(&w, &m, panel).unwrap();
    let g = wingcut::gcode::emit(&p, &m, "taper");
    let (s_lo, s_hi) = (panel.block.s_root, panel.block.s_root + 1000.0);
    let mut pos = [f64::NAN; 4];
    let mut g93 = false;
    let mut cuts = 0;
    for line in g.lines() {
        let ws = words(line);
        if ws.contains(&('G', 93.0)) {
            g93 = true;
        }
        if ws.contains(&('G', 94.0)) {
            g93 = false;
        }
        let mut new = pos;
        let mut f = None;
        for (c, v) in &ws {
            match c {
                'X' => new[0] = *v,
                'Y' => new[1] = *v,
                'Z' => new[2] = *v,
                'A' => new[3] = *v,
                'F' => f = Some(*v),
                _ => {}
            }
        }
        if g93 && ws.contains(&('G', 1.0)) {
            cuts += 1;
            let t = 1.0 / f.expect("every G93 move needs F");
            let rt0 = P2::new(pos[0], pos[1]);
            let rt1 = P2::new(new[0], new[1]);
            let tt0 = P2::new(pos[2], pos[3]);
            let tt1 = P2::new(new[2], new[3]);
            // One line moves both towers, so both take exactly t. Check that
            // t is the time of the faster block face at feed_cut.
            let face = |r: P2, tp: P2, s: f64| r + (tp - r) * (s / m.tower_spacing);
            let la = face(rt0, tt0, s_lo).dist(face(rt1, tt1, s_lo));
            let lb = face(rt0, tt0, s_hi).dist(face(rt1, tt1, s_hi));
            let expect = la.max(lb) / m.feed_cut;
            // Tolerance: 3-decimal positions (~0.004 mm of length) and the
            // 3-decimal F word (dt = t^2 * 0.0005).
            assert!(
                (t - expect).abs() <= 0.004 / m.feed_cut + t * t * 0.0005 + 1e-9,
                "line {line:?}: t {t} vs {expect}"
            );
            // Both tower speeds follow from the same t.
            let (vr, vt) = (rt0.dist(rt1) / t, tt0.dist(tt1) / t);
            assert!(vr.is_finite() && vt.is_finite());
        }
        pos = new;
    }
    assert!(cuts > 100);
    // In-memory moves: per-segment time = faster face / feed, exactly.
    let fp = &p.faces;
    let mut k = 0;
    for mv in &p.moves {
        if let Move::Cut { minutes, .. } = mv {
            let la = fp[0].path[k].dist(fp[0].path[k + 1]);
            let lb = fp[1].path[k].dist(fp[1].path[k + 1]);
            assert!((minutes - la.max(lb) / m.feed_cut).abs() < 1e-12);
            k += 1;
        }
    }
}

#[test]
fn adjacent_panels_share_an_identical_joint_section() {
    let m = machine("");
    let text = std::fs::read_to_string(examples().join("wing.toml")).unwrap();
    // Cutouts are per panel (spars on the centre, hinge on the tip), so the
    // shared joint is the outer section: compare without them.
    let mut in_cutout = false;
    let text: String = text
        .lines()
        .filter(|l| {
            if l.starts_with("[[panel.cutout]]") {
                in_cutout = true;
            } else if l.starts_with('[') {
                in_cutout = false;
            }
            !in_cutout
        })
        .map(|l| format!("{l}\n"))
        .collect();
    assert!(!text.contains("surface ="));
    let w = model(&text, &m);
    let te = m.te_min_thickness;
    // Wing-coordinate sections at the joint are bit-identical.
    for (a, b) in [("center-L", "tip-L"), ("center-R", "tip-R")] {
        let (pa, pb) = (w.wing.panel(a).unwrap(), w.wing.panel(b).unwrap());
        assert_eq!(pa.to, pb.from);
        assert_eq!(w.section_at(pa.to, te), w.section_at(pb.from, te));
    }
    // And on the machine: the nominal profile of center-L's tip face equals
    // tip-L's root face up to the block placement (a pure translation).
    // With sheeting the two panels sample the joint at slightly different
    // points (each panel pairs it with its other face), so compare shapes;
    // without sheeting the points themselves are identical.
    let unsheeted: String = {
        let mut skip = false;
        text.lines()
            .filter(|l| {
                if l.starts_with("[panel.sheeting]") {
                    skip = true;
                } else if l.starts_with('[') {
                    skip = false;
                }
                !skip
            })
            .map(|l| format!("{l}\n"))
            .collect()
    };
    assert!(text.contains("[panel.sheeting]") && !unsheeted.contains("front ="));
    // A uniform skin: a step between the front and rear skin becomes a
    // one-segment ramp, which the two panels may place slightly apart.
    let uniform = text.replace("front = 1.5", "front = 0.6");
    assert_ne!(uniform, text);
    for (wing_text, exact) in [(&uniform, false), (&unsheeted, true)] {
        let w = model(wing_text, &m);
        for (a, b, ia, ib) in [("center-L", "tip-L", 1, 0), ("center-R", "tip-R", 0, 1)] {
            let (c, t) = (plan(&w, &m, a), plan(&w, &m, b));
            let (fa, fb) = (&c.faces[ia], &t.faces[ib]);
            assert!(!fa.is_wing_root && fb.is_wing_root);
            let d = fb.nominal[0] - fa.nominal[0];
            if exact {
                assert_eq!(fa.nominal.len(), fb.nominal.len());
                for (p, q) in fa.nominal.iter().zip(&fb.nominal) {
                    assert!((*p + d).dist(*q) < 1e-9);
                }
            } else {
                for p in &fa.nominal {
                    let gap = dist_to(&fb.nominal, *p + d);
                    // Below the machine's step size (1/80 mm).
                    assert!(gap < 0.01, "{a}/{b}: joint differs by {gap} mm");
                }
            }
        }
    }
}

/// Distance from `q` to the polyline `line`.
fn dist_to(line: &[P2], q: P2) -> f64 {
    line.windows(2)
        .map(|s| {
            let (a, b) = (s[0], s[1]);
            let ab = b - a;
            let t = if ab.dot(ab) > 0.0 {
                ((q - a).dot(ab) / ab.dot(ab)).clamp(0.0, 1.0)
            } else {
                0.0
            };
            q.dist(a + ab * t)
        })
        .fold(f64::MAX, f64::min)
}

#[test]
fn limit_violations_are_reported() {
    let m = machine("limits = { x = [0, 1000], y = [50, 300] }");
    let w = model(TAPER, &m);
    let err = plan_panel(&w, &m, w.wing.panel("mid").unwrap())
        .unwrap_err()
        .to_string();
    assert!(err.contains("axis limit violation"), "{err}");
    assert!(err.contains("outside [50, 300]"), "{err}");
    assert!(err.contains("tower Y") || err.contains("tower A"), "{err}");

    let m = machine("limits = { x = [0, 200], y = [0, 300] }");
    let err = plan_panel(&w, &m, w.wing.panel("mid").unwrap())
        .unwrap_err()
        .to_string();
    assert!(err.contains("outside [0, 200]"), "{err}");
    assert!(err.contains("surface point"), "{err}");

    // Custom axis letters show up in the message.
    let m =
        machine("axes = [\"X\", \"Y\", \"U\", \"V\"]\nlimits = { x = [0, 1000], y = [50, 300] }");
    let err = plan_panel(&w, &m, w.wing.panel("mid").unwrap())
        .unwrap_err()
        .to_string();
    assert!(err.contains("tower V") || err.contains("tower Y"), "{err}");
}

#[test]
fn block_outside_the_towers_is_rejected() {
    let m = machine("");
    let bad = TAPER.replace("s_root = 300", "s_root = 700");
    let w = model(&bad, &m);
    let err = plan_panel(&w, &m, w.wing.panel("mid").unwrap()).unwrap_err();
    assert!(err.to_string().contains("between the towers"));
}

#[test]
fn mirrored_panel_swaps_the_faces() {
    let m = machine("");
    let right = TAPER.replace(
        "block = { s_root = 300,",
        "side = \"right\"\nblock = { s_root = 300,",
    );
    let (wl, wr) = (model(TAPER, &m), model(&right, &m));
    let (l, r) = (plan(&wl, &m, "mid"), plan(&wr, &m, "mid"));
    assert!(l.faces[0].is_wing_root && !r.faces[0].is_wing_root);
    assert_paths_eq(&l.faces[0].nominal, &r.faces[1].nominal, 1e-9);
    assert_paths_eq(&l.faces[1].nominal, &r.faces[0].nominal, 1e-9);
    assert_eq!(l.faces[0].kerf, r.faces[1].kerf);
}

#[test]
fn slower_face_gets_more_kerf_and_kerf_is_outward() {
    let m = machine("");
    let w = model(TAPER, &m);
    let p = plan(&w, &m, "mid");
    let [root, tip] = &p.faces;
    assert!(root.speed_avg > tip.speed_avg);
    assert!(tip.kerf > root.kerf);
    // The faster face runs at feed_cut on its longest segments.
    assert!((root.speed_max - m.feed_cut).abs() < 1e-6);
    // Outward: the kerfed profile encloses the nominal one, ~kerf/2 away.
    for f in [root, tip] {
        let area = |v: &[P2]| wingcut::geom::signed_area(v).abs();
        assert!(area(&f.kerfed) > area(&f.nominal));
        let n = f.nominal.len();
        let mid = n / 4;
        let d = f.nominal[mid].dist(f.kerfed[mid]);
        assert!((d - f.kerf / 2.0).abs() < 0.01, "{d} vs {}", f.kerf / 2.0);
    }
}

#[test]
fn cut_path_is_one_line_from_the_front_and_back() {
    let m = machine("");
    let w = model(TAPER, &m);
    let p = plan(&w, &m, "mid");
    let x_out = 100.0 - m.lead_clearance;
    for f in &p.faces {
        let (a, b) = (f.path[0], f.path[f.path.len() - 1]);
        assert_eq!((a.x, b.x), (x_out, x_out));
        // Entry above exit: the top and bottom beds stay whole.
        assert!(a.y > b.y);
    }
    // Wire on before the first cut, off after the last one.
    let pos = |pred: fn(&Move) -> bool| p.moves.iter().position(pred).unwrap();
    let first_cut = pos(|m| matches!(m, Move::Cut { .. }));
    let last_cut = p.moves.len()
        - 1
        - p.moves
            .iter()
            .rev()
            .position(|m| matches!(m, Move::Cut { .. }))
            .unwrap();
    assert!(pos(|m| matches!(m, Move::HeatOn)) < first_cut);
    assert!(pos(|m| matches!(m, Move::HeatOff)) > last_cut);
}

#[test]
fn warns_about_interior_stations_and_oversize_profiles() {
    let m = machine("");
    let three = r#"
name = "w"
[[station]]
span = 0
chord = 200
airfoil = "airfoils/naca2412.dat"
[[station]]
span = 500
chord = 180
airfoil = "airfoils/naca2412.dat"
[[station]]
span = 1000
chord = 150
airfoil = "airfoils/naca2412.dat"
[[panel]]
name = "p"
from = 0
to = 1000
block = { s_root = 300, x0 = 100, y0 = 20, height = 10, width = 300 }
"#;
    let w = model(three, &m);
    let p = plan(&w, &m, "p");
    assert!(p.warnings.iter().any(|w| w.contains("station at span 500")));
    assert!(p.warnings.iter().any(|w| w.contains("does not fit")));
}

#[test]
fn le_dwell_is_emitted_at_the_leading_edge() {
    let m = machine("le_dwell_s = 1.5");
    let w = model(RECT, &m);
    let p = plan(&w, &m, "mid");
    let i = p
        .moves
        .iter()
        .position(|m| {
            matches!(
                m,
                Move::Dwell {
                    note: "leading edge",
                    ..
                }
            )
        })
        .unwrap();
    let Move::Cut { root, .. } = p.moves[i - 1] else {
        panic!("dwell must follow a cut move")
    };
    let max_x = p.root_tower.iter().map(|q| q.x).fold(f64::MIN, f64::max);
    assert!(
        (root.x - max_x).abs() < 1e-6,
        "LE is the most forward point"
    );
}

#[test]
fn limit_errors_are_typed_for_callers() {
    use wingcut::cut::LimitError;
    let m = machine("limits = { x = [0, 1000], y = [50, 300] }");
    let w = model(TAPER, &m);
    let err = plan_panel(&w, &m, w.wing.panel("mid").unwrap()).unwrap_err();
    let le = err.downcast_ref::<LimitError>().expect("a LimitError");
    assert_eq!(le.panel, "mid");
    assert!(!le.violations.is_empty());
    for v in &le.violations {
        assert!(v.value < 50.0);
        assert_eq!(v.limits, [50.0, 300.0]);
        assert!(matches!(v.axis.as_str(), "Y" | "A"));
        assert!(v.tower == "root" || v.tower == "tip");
        assert!(v.index.unwrap() < 2 * w.n + 1);
    }
}

/// Small deterministic PRNG so the sweep needs no extra dependency.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next()
    }
}

/// Invariants that must hold for any sane wing: finite output, sync kept,
/// faster face at feed_cut, kerf outside the profile, no self-intersection.
#[test]
fn invariants_hold_across_random_wings() {
    let m = machine("max_segment = 1.0\nlimits = { x = [-2000, 3000], y = [-2000, 2000] }");
    let mut rng = Lcg(42);
    for case in 0..40 {
        let (c0, c1) = (rng.range(120.0, 400.0), rng.range(60.0, 400.0));
        let span = rng.range(300.0, 1600.0);
        let s_root = rng.range(0.0, 1600.0 - span);
        let side = if rng.next() < 0.5 { "left" } else { "right" };
        let mut cutouts = String::new();
        if rng.next() < 0.6 {
            cutouts += &format!(
                "[[panel.cutout]]\nsurface = \"upper\"\nx = {}\nwidth = {}\ndepth = {}\n",
                rng.range(0.25, 0.4),
                rng.range(4.0, 10.0),
                rng.range(0.5, 2.0)
            );
        }
        if rng.next() < 0.6 {
            // Hinge from either side, as a slot or a V.
            let surface = if rng.next() < 0.5 { "lower" } else { "upper" };
            let shape = if rng.next() < 0.5 {
                format!("width = {}", rng.range(0.5, 2.0))
            } else {
                format!("angle = {}", rng.range(20.0, 40.0))
            };
            cutouts += &format!(
                "[[panel.cutout]]\nsurface = \"{surface}\"\nx = {}\n{shape}\nleave = {}\n",
                rng.range(0.65, 0.78),
                rng.range(0.3, 1.0)
            );
        }
        if rng.next() < 0.6 {
            cutouts += &format!(
                "[panel.sheeting]\ntop = {}\nbottom = {}\nfront = {}\nfront_to = {}\nle_cap = {}\nte_stock = {}\n",
                rng.range(0.0, 1.0),
                rng.range(0.0, 1.0),
                rng.range(0.0, 1.5),
                rng.range(0.1, 0.4),
                if rng.next() < 0.3 {
                    rng.range(0.01, 0.05)
                } else {
                    0.0
                },
                if rng.next() < 0.3 {
                    rng.range(0.85, 0.95)
                } else {
                    1.0
                },
            );
        }
        let wing = format!(
            r#"
name = "r"
[[station]]
span = 0
chord = {c0}
airfoil = "airfoils/naca2412.dat"
twist_deg = {t0}
[[station]]
span = {span}
chord = {c1}
airfoil = "airfoils/naca2410.dat"
le_offset = {le}
dihedral_offset = {dh}
twist_deg = {t1}
twist_pivot = {pv}
[[panel]]
name = "p"
from = 0
to = {span}
side = "{side}"
block = {{ s_root = {s_root}, x0 = 100, y0 = 0, height = 200, width = 600 }}
{cutouts}"#,
            t0 = rng.range(-3.0, 3.0),
            t1 = rng.range(-5.0, 3.0),
            le = rng.range(-50.0, 150.0),
            dh = rng.range(-60.0, 60.0),
            pv = rng.range(0.0, 1.0),
        );
        if std::env::var("DUMP_CASE").ok().as_deref() == Some(&case.to_string()) {
            eprintln!("{wing}");
        }
        let w = model(&wing, &m);
        let p = plan(&w, &m, "p");
        let n_path = p.faces[0].path.len();
        if cutouts.is_empty() {
            assert_eq!(n_path, 2 * w.n + 1, "case {case}");
        }
        assert_eq!(p.root_tower.len(), n_path, "case {case}");
        assert_eq!(p.tip_tower.len(), n_path);
        for f in &p.faces {
            assert_eq!(f.path.len(), n_path);
            assert!(f.path.iter().all(|q| q.x.is_finite() && q.y.is_finite()));
            assert!(f.kerf > 0.0 && f.speed_avg > 0.0 && f.speed_avg <= m.feed_cut + 1e-9);
            let area = |v: &[P2]| wingcut::geom::signed_area(v).abs();
            assert!(
                area(&f.kerfed) > area(&f.nominal),
                "case {case}: kerf not outward"
            );
            assert_no_self_intersection(&f.kerfed, case);
        }
        // Segments where both faces stand still (collapsed slits) are
        // skipped by the planner; every other segment is one G93 move.
        let fp = &p.faces;
        let segs: Vec<(f64, f64)> = (0..n_path - 1)
            .map(|k| {
                (
                    fp[0].path[k].dist(fp[0].path[k + 1]),
                    fp[1].path[k].dist(fp[1].path[k + 1]),
                )
            })
            .filter(|&(la, lb)| la >= 1e-9 || lb >= 1e-9)
            .collect();
        let cuts: Vec<f64> = p
            .moves
            .iter()
            .filter_map(|mv| match mv {
                Move::Cut { minutes, root, tip } => {
                    assert!(root.x.is_finite() && tip.y.is_finite());
                    Some(*minutes)
                }
                _ => None,
            })
            .collect();
        assert_eq!(cuts.len(), segs.len(), "case {case}");
        // Slots narrower than the kerf may pause one face for a segment or
        // two (the planner warns about those); anything else must not.
        if !p
            .warnings
            .iter()
            .any(|w| w.contains("narrower than the kerf"))
        {
            assert_eq!(one_sided_pauses(&p), 0, "case {case}");
        }
        for (t, (la, lb)) in cuts.iter().zip(&segs) {
            assert!(t.is_finite() && *t > 0.0, "case {case}");
            assert!((la.max(*lb) / t - m.feed_cut).abs() < 1e-6, "case {case}");
        }
    }
}

fn assert_no_self_intersection(v: &[P2], case: usize) {
    use wingcut::geom::seg_intersect;
    for i in 0..v.len() - 1 {
        for j in i + 2..v.len() - 1 {
            if let Some(x) = seg_intersect(v[i], v[i + 1], v[j], v[j + 1]) {
                // Touching at a shared (collapsed) vertex is fine.
                // (With TE stock the path also starts and ends at one point.)
                let shared = [v[i], v[i + 1], v[j], v[j + 1]]
                    .iter()
                    .any(|q| q.dist(x) < 1e-9);
                assert!(
                    shared,
                    "case {case}: kerfed path crosses itself at {i}/{j}: {:?} {:?} / {:?} {:?}",
                    v[i],
                    v[i + 1],
                    v[j],
                    v[j + 1]
                );
            }
        }
    }
}

#[test]
fn spar_slot_is_cut_to_size_after_kerf() {
    let m = machine("");
    let wing = RECT.replace(
        "block = { s_root = 300, x0 = 100, y0 = 20, height = 60, width = 300 }",
        "block = { s_root = 300, x0 = 100, y0 = 20, height = 60, width = 300 }\n[[panel.cutout]]\nsurface = \"upper\"\nx = 0.3\nwidth = 10\ndepth = 3",
    );
    let w = model(&wing, &m);
    let p = plan(&w, &m, "mid");
    for f in &p.faces {
        // The slot floor is the one horizontal stretch 3 mm below the top.
        let floor: Vec<(P2, P2)> = f
            .kerfed
            .windows(2)
            .filter(|s| (s[0].y - s[1].y).abs() < 1e-9 && (s[0].x - s[1].x).abs() > 5.0)
            .map(|s| (s[0], s[1]))
            .collect();
        assert_eq!(floor.len(), 1, "{floor:?}");
        let (a, b) = floor[0];
        // Wire path is narrower and shallower by kerf/2 per side, so the
        // slot in the foam is exactly 10 x 3 mm.
        assert!(((a.x - b.x).abs() - (10.0 - f.kerf)).abs() < 1e-6);
        let nominal_floor = f
            .nominal
            .windows(2)
            .find(|s| {
                (s[0].y - s[1].y).abs() < 1e-9 && ((s[0].x - s[1].x).abs() - 10.0).abs() < 1e-6
            })
            .expect("nominal floor");
        assert!((a.y - (nominal_floor[0].y + f.kerf / 2.0)).abs() < 1e-6);
    }
    // Root and tip are identical (rectangular wing), cutout included.
    assert_paths_eq(&p.root_tower, &p.tip_tower, 1e-9);
}

#[test]
fn impossible_cutout_is_an_error_naming_the_panel() {
    let m = machine("");
    let wing = RECT.replace(
        "block = { s_root = 300, x0 = 100, y0 = 20, height = 60, width = 300 }",
        "block = { s_root = 300, x0 = 100, y0 = 20, height = 60, width = 300 }\n[[panel.cutout]]\nsurface = \"upper\"\nx = 0.3\nwidth = 10\ndepth = 50",
    );
    let w = model(&wing, &m);
    let err = format!(
        "{:#}",
        plan_panel(&w, &m, w.wing.panel("mid").unwrap()).unwrap_err()
    );
    assert!(
        err.contains("panel \"mid\"") && err.contains("goes through the section"),
        "{err}"
    );
}

#[test]
fn v_hinge_wire_stops_half_a_kerf_short_of_the_apex() {
    let m = machine("");
    let wing = RECT.replace(
        "block = { s_root = 300, x0 = 100, y0 = 20, height = 60, width = 300 }",
        "block = { s_root = 300, x0 = 100, y0 = 20, height = 60, width = 300 }\n[[panel.cutout]]\nsurface = \"lower\"\nx = 0.75\nangle = 30\nleave = 0.5",
    );
    let w = model(&wing, &m);
    let p = plan(&w, &m, "mid");
    for f in &p.faces {
        // The apex is the highest point of the lower surface's V.
        let j = (1..f.nominal.len() - 1)
            .find(|&i| {
                let (a, b, c) = (f.nominal[i - 1], f.nominal[i], f.nominal[i + 1]);
                b.y > a.y + 1.0 && b.y > c.y + 1.0
            })
            .expect("an apex");
        let d = f.kerfed[j].dist(f.nominal[j]);
        assert!((d - f.kerf / 2.0).abs() < 1e-9, "{d}");
        assert!(f.kerfed[j].y < f.nominal[j].y, "wire is inside the groove");
    }
}

#[test]
fn sheeting_shrinks_the_core_and_keeps_faces_in_sync() {
    let m = machine("");
    let base = RECT.replace(
        "block = { s_root = 300, x0 = 100, y0 = 20, height = 60, width = 300 }",
        "block = { s_root = 300, x0 = 100, y0 = 20, height = 60, width = 300 }\n[panel.sheeting]\ntop = 1.0\nbottom = 0.8\nfront = 1.5\nle_cap = 0.02\nte_stock = 0.9",
    );
    let (bare, sheeted) = (model(RECT, &m), model(&base, &m));
    let (a, b) = (plan(&bare, &m, "mid"), plan(&sheeted, &m, "mid"));
    let area = |v: &[P2]| wingcut::geom::signed_area(v).abs();
    assert!(area(&b.faces[0].nominal) < area(&a.faces[0].nominal) * 0.9);
    assert_eq!(b.faces[0].path.len(), b.faces[1].path.len());
    // Rectangular wing: both towers still identical.
    assert_paths_eq(&b.root_tower, &b.tip_tower, 1e-9);
}

/// Count segments where one face moves and the other stands still (the
/// still end of the wire would dwell and over-melt).
fn one_sided_pauses(p: &CutPlan) -> usize {
    let (a, b) = (&p.faces[0].path, &p.faces[1].path);
    (0..a.len() - 1)
        .filter(|&i| (a[i].dist(a[i + 1]) < 1e-9) != (b[i].dist(b[i + 1]) < 1e-9))
        .count()
}

#[test]
fn example_panels_never_pause_one_end_of_the_wire() {
    let m = Machine::load(&examples().join("machine.toml")).unwrap();
    let w = WingModel::new(
        Wing::load(&examples().join("wing.toml")).unwrap(),
        m.max_segment,
    )
    .unwrap();
    for name in ["center-L", "tip-L", "center-R", "tip-R"] {
        let p = plan(&w, &m, name);
        assert_eq!(one_sided_pauses(&p), 0, "{name}");
        for f in &p.faces {
            assert!(f.speed_min > 0.0, "{name}: a face stands still somewhere");
        }
    }
}
