//! Config validation: every bad input must fail early with a clear message.

use std::path::Path;
use wingcut::config::{Machine, Wing};

fn machine_err(toml: &str) -> String {
    format!("{:#}", Machine::parse(toml).unwrap_err())
}

fn wing_err(toml: &str) -> String {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    format!("{:#}", Wing::parse(toml, &base).unwrap_err())
}

#[test]
fn empty_machine_config_uses_spec_defaults() {
    let m = Machine::parse("").unwrap();
    assert_eq!(m.tower_spacing, 1600.0);
    assert_eq!(m.axes, ["X", "Y", "Z", "A"].map(String::from));
    assert_eq!(m.limits.x, [0.0, 1000.0]);
    assert_eq!(m.heat_on, "M3 S1000");
    assert_eq!(m.te_min_thickness, 0.8);
}

#[test]
fn machine_rejects_non_finite_numbers() {
    assert!(machine_err("feed_cut = nan").contains("feed_cut must be a finite number"));
    assert!(machine_err("tower_spacing = inf").contains("tower_spacing must be a finite"));
    assert!(machine_err("limits = { x = [0, inf], y = [0, 300] }").contains("limits.x[1]"));
    assert!(machine_err("kerf = [ { feed = nan, kerf = 1 } ]").contains("kerf.feed"));
}

#[test]
fn machine_rejects_bad_values() {
    assert!(machine_err("feed_cut = 0").contains("feed_cut must be > 0"));
    assert!(machine_err("limits = { x = [10, 0], y = [0, 300] }").contains("min < max"));
    assert!(machine_err("kerf = []").contains("kerf table is empty"));
    assert!(machine_err("kerf = [ { feed = 100, kerf = -1 } ]").contains("bad kerf entry"));
    assert!(machine_err("miter_limit = 0.5").contains("miter_limit"));
}

#[test]
fn machine_rejects_bad_axis_letters() {
    assert!(machine_err(r#"axes = ["X", "Y", "X", "A"]"#).contains("used twice"));
    assert!(machine_err(r#"axes = ["X", "Y", "x", "A"]"#).contains("used twice"));
    assert!(machine_err(r#"axes = ["X", "Y", "F", "A"]"#).contains("clashes with a G-code word"));
    assert!(machine_err(r#"axes = ["X", "Y", "ZZ", "A"]"#).contains("single letter"));
}

#[test]
fn machine_rejects_multi_line_heat_commands() {
    let e = machine_err("heat_on = \"M3 S1000\\nG0 X0\"");
    assert!(e.contains("heat_on must be one non-empty line"), "{e}");
    assert!(machine_err("heat_off = \"\"").contains("heat_off"));
}

#[test]
fn machine_rejects_unknown_keys() {
    // A typo must not silently fall back to a default.
    assert!(machine_err("feed_cutt = 200").contains("unknown field"));
}

const STATIONS: &str = r#"
name = "w"
[[station]]
span = 0
chord = 200
airfoil = "airfoils/naca2412.dat"
[[station]]
span = 1000
chord = 150
airfoil = "airfoils/naca2410.dat"
"#;

fn panel(fields: &str) -> String {
    format!(
        "{STATIONS}\n[[panel]]\nname = \"p\"\n{fields}\nblock = {{ s_root = 300, x0 = 100, y0 = 20, height = 60 }}\n"
    )
}

#[test]
fn wing_accepts_the_shipped_example() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    let text = std::fs::read_to_string(base.join("wing.toml")).unwrap();
    let w = Wing::parse(&text, &base).unwrap();
    assert_eq!(w.panels.len(), 4);
}

#[test]
fn wing_rejects_non_finite_numbers() {
    let e = wing_err(&STATIONS.replace("chord = 150", "chord = nan"));
    assert!(e.contains("station chord must be a finite number"), "{e}");
    let e = wing_err(&panel("from = 0\nto = inf"));
    assert!(e.contains("\"p\" to must be a finite"), "{e}");
}

#[test]
fn wing_rejects_bad_geometry() {
    assert!(
        wing_err(&STATIONS.replace("span = 1000", "span = 0")).contains("two stations at span 0")
    );
    assert!(wing_err(&STATIONS.replace("chord = 150", "chord = 0")).contains("chord must be > 0"));
    assert!(wing_err(&panel("from = 0\nto = 1200")).contains("inside the stations"));
    assert!(wing_err(&panel("from = 500\nto = 500")).contains("must be increasing"));
    let e = wing_err(
        &panel("from = 0\nto = 1000").replace("s_root = 300,", "s_root = 300, length = 900,"),
    );
    assert!(e.contains("must equal the panel span"), "{e}");
    let one = STATIONS
        .split("[[station]]")
        .take(2)
        .collect::<Vec<_>>()
        .join("[[station]]");
    assert!(wing_err(&one).contains("at least two"));
}

#[test]
fn wing_rejects_bad_panel_names() {
    let p = panel("from = 0\nto = 1000");
    let dup = format!("{p}\n{}", p.replace(STATIONS, ""));
    assert!(wing_err(&dup).contains("duplicate panel name"));
    assert!(
        wing_err(&p.replace("name = \"p\"", "name = \"../evil\"")).contains("must be non-empty")
    );
}

#[test]
fn wing_rejects_unknown_keys_and_units() {
    assert!(wing_err(&panel("from = 0\nto = 1000\nsidee = \"left\"")).contains("unknown field"));
    assert!(
        wing_err(&STATIONS.replace("name = \"w\"", "name = \"w\"\nunits = \"in\""))
            .contains("units")
    );
}

#[test]
fn configs_round_trip_through_toml() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    let m = Machine::load(&base.join("machine.toml")).unwrap();
    assert_eq!(Machine::parse(&m.to_toml().unwrap()).unwrap(), m);
    let w = Wing::load(&base.join("wing.toml")).unwrap();
    let text = w.to_toml().unwrap();
    assert!(text.contains("[[panel.cutout]]") && text.contains("[[station]]"));
    assert_eq!(Wing::parse(&text, &base).unwrap(), w);
}

#[test]
fn cutouts_are_validated() {
    let p = panel("from = 0\nto = 1000");
    let cut = |body: &str| format!("{p}\n[[panel.cutout]]\nsurface = \"upper\"\n{body}\n");
    assert!(wing_err(&cut("x = 0.3\nwidth = 10")).contains("exactly one of depth"));
    assert!(
        wing_err(&cut("x = 0.3\nwidth = 10\ndepth = 2\nleave = 1"))
            .contains("exactly one of depth")
    );
    assert!(wing_err(&cut("x = 1.2\nwidth = 10\ndepth = 2")).contains("chord fraction"));
    assert!(wing_err(&cut("x = 0.3\nwidth = 0\ndepth = 2")).contains("width must be > 0"));
    assert!(wing_err(&cut("x = 0.3\nwidth = 10\ndepth = nan")).contains("finite"));
    assert!(
        wing_err(&format!(
            "{p}\n[[panel.cutout]]\nsurface = \"side\"\nx = 0.3\nwidth = 1\ndepth = 1\n"
        ))
        .contains("unknown variant")
    );
}

#[test]
fn sheeting_and_v_grooves_are_validated() {
    let p = panel("from = 0\nto = 1000");
    let sh = |body: &str| format!("{p}\n[panel.sheeting]\n{body}\n");
    assert!(wing_err(&sh("top = -1")).contains("thicknesses must be >= 0"));
    assert!(wing_err(&sh("front = nan")).contains("sheeting front must be a finite"));
    assert!(wing_err(&sh("front_to = 1.5")).contains("front_to"));
    assert!(wing_err(&sh("le_cap = 0.5\nte_stock = 0.4")).contains("le_cap < te_stock"));
    assert!(wing_err(&sh("tops = 1")).contains("unknown field"));
    let cut = |body: &str| {
        format!("{p}\n[[panel.cutout]]\nsurface = \"lower\"\nx = 0.75\nleave = 0.5\n{body}\n")
    };
    assert!(wing_err(&cut("width = 1\nangle = 30")).contains("exactly one of width"));
    assert!(wing_err(&cut("angle = 190")).contains("between 0 and 180"));
    assert!(wing_err(&cut("")).contains("exactly one of width"));
}

#[test]
fn sheeting_defaults_and_round_trip() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    let p = panel("from = 0\nto = 1000");
    let text = format!(
        "{p}\n[panel.sheeting]\ntop = 1.0\n\n[[panel.cutout]]\nsurface = \"lower\"\nx = 0.75\nangle = 30\nleave = 0.5\n"
    );
    let w = Wing::parse(&text, &base).unwrap();
    let sh = &w.panels[0].sheeting;
    assert_eq!(
        (
            sh.top,
            sh.bottom,
            sh.front,
            sh.front_to,
            sh.le_cap,
            sh.te_stock
        ),
        (1.0, 0.0, 0.0, 0.30, 0.0, 1.0)
    );
    assert_eq!(Wing::parse(&w.to_toml().unwrap(), &base).unwrap(), w);
    // Unsheeted panels don't write an empty [panel.sheeting] table.
    let plain = Wing::parse(&panel("from = 0\nto = 1000"), &base).unwrap();
    assert!(!plain.to_toml().unwrap().contains("sheeting"));
}
