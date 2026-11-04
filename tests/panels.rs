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
    let c = plan(&w, &m, "center-L");
    let t = plan(&w, &m, "tip-L");
    let (fa, fb) = (&c.faces[1], &t.faces[0]);
    assert!(!fa.is_wing_root && fb.is_wing_root);
    let d = fb.nominal[0] - fa.nominal[0];
    for (p, q) in fa.nominal.iter().zip(&fb.nominal) {
        assert!((*p + d).dist(*q) < 1e-9);
    }
    // Same for the mirrored side.
    let c = plan(&w, &m, "center-R");
    let t = plan(&w, &m, "tip-R");
    let (fa, fb) = (&c.faces[0], &t.faces[1]);
    let d = fb.nominal[0] - fa.nominal[0];
    for (p, q) in fa.nominal.iter().zip(&fb.nominal) {
        assert!((*p + d).dist(*q) < 1e-9);
    }
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
