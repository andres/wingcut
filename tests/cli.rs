//! Runs the real binary on the shipped examples.

use std::path::Path;
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_wingcut"))
}

fn ex(f: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join(f)
        .to_string_lossy()
        .into_owned()
}

#[test]
fn four_metre_wing_generates_in_one_command() {
    let out = std::env::temp_dir().join(format!("wingcut-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let r = bin()
        .args([
            "gen",
            "--machine",
            &ex("machine.toml"),
            "--wing",
            &ex("wing.toml"),
            "-o",
        ])
        .arg(&out)
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let stdout = String::from_utf8_lossy(&r.stdout);
    assert!(!stdout.contains("warning"), "{stdout}");
    for p in ["center-L", "tip-L", "center-R", "tip-R"] {
        let g = std::fs::read_to_string(out.join(format!("{p}.gcode"))).unwrap();
        check_gcode(&g);
        let s = std::fs::read_to_string(out.join(format!("{p}.svg"))).unwrap();
        assert!(s.starts_with("<svg") && s.trim_end().ends_with("</svg>"));
    }
    std::fs::remove_dir_all(&out).unwrap();
}

/// Structural validity of the emitted program.
fn check_gcode(g: &str) {
    let lines: Vec<&str> = g.lines().collect();
    assert!(g.is_ascii());
    assert!(lines.iter().any(|l| l.starts_with("G21 G90")));
    let i93 = lines.iter().position(|l| *l == "G93").unwrap();
    let i94 = lines.iter().rposition(|l| *l == "G94").unwrap();
    assert!(i94 > i93);
    let on = lines.iter().position(|l| l.starts_with("M3")).unwrap();
    assert!(on < i93);
    assert_eq!(lines.last(), Some(&"M2"));
    for l in &lines[i93 + 1..i94] {
        if l.starts_with("G1") {
            let w: Vec<&str> = l.split_whitespace().collect();
            assert_eq!(w.len(), 6, "{l}");
            assert!(w[5].starts_with('F'));
        }
    }
    for l in lines.iter().filter(|l| !l.starts_with(';')) {
        let code = l.split(';').next().unwrap();
        assert!(
            !code.contains(['e', 'E']) && !code.contains("NaN") && !code.contains("inf"),
            "{l}"
        );
        for w in code.split_whitespace().skip(1) {
            if let Some((_, num)) = w.split_at_checked(1)
                && let Some((_, dec)) = num.split_once('.')
            {
                assert_eq!(dec.len(), 3, "{l}");
            }
        }
    }
}

#[test]
fn check_and_single_panel_and_info() {
    let r = bin()
        .args([
            "check",
            "--machine",
            &ex("machine.toml"),
            "--wing",
            &ex("wing.toml"),
            "--panel",
            "tip-R",
        ])
        .output()
        .unwrap();
    assert!(r.status.success());
    let s = String::from_utf8_lossy(&r.stdout);
    assert!(s.contains("panel tip-R") && !s.contains("center-L") && s.contains("ok: 1 panel"));

    let r = bin()
        .args([
            "check",
            "--machine",
            &ex("machine.toml"),
            "--wing",
            &ex("wing.toml"),
            "--panel",
            "nope",
        ])
        .output()
        .unwrap();
    assert!(!r.status.success());
    assert!(String::from_utf8_lossy(&r.stderr).contains("no panel named"));

    let r = bin()
        .args(["info", &ex("airfoils/naca2412.dat")])
        .output()
        .unwrap();
    assert!(r.status.success());
    let s = String::from_utf8_lossy(&r.stdout);
    assert!(s.contains("NACA 2412") && s.contains("Selig") && s.contains("12.00 %"));
}
