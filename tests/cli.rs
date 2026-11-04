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
    let stderr = String::from_utf8_lossy(&r.stderr);
    assert!(
        stderr.is_empty(),
        "the example must generate without warnings: {stderr}"
    );
    for p in ["center-L", "tip-L", "center-R", "tip-R"] {
        let g = std::fs::read_to_string(out.join(format!("{p}.gcode"))).unwrap();
        check_gcode(&g);
        let s = std::fs::read_to_string(out.join(format!("{p}.svg"))).unwrap();
        assert!(s.starts_with("<svg") && s.trim_end().ends_with("</svg>"));
    }
    let leftovers: Vec<_> = std::fs::read_dir(&out)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "temporary files left behind");
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

fn temp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("wingcut-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn warnings_go_to_stderr_and_results_to_stdout() {
    let dir = temp("warn");
    let wing = std::fs::read_to_string(ex("wing.toml"))
        .unwrap()
        // Forward slashes: a Windows path's backslashes are escapes in TOML.
        .replace(
            "airfoils/",
            &format!("{}/", ex("airfoils").replace('\\', "/")),
        )
        .replace("width = 300", "width = 100");
    let wing_path = dir.join("wing.toml");
    std::fs::write(&wing_path, wing).unwrap();
    let r = bin()
        .args(["check", "--machine", &ex("machine.toml"), "--wing"])
        .arg(&wing_path)
        .output()
        .unwrap();
    let (out, err) = (
        String::from_utf8_lossy(&r.stdout),
        String::from_utf8_lossy(&r.stderr),
    );
    assert!(r.status.success(), "{err}");
    assert!(
        err.contains("warning: panel tip-L:") && err.contains("does not fit"),
        "{err}"
    );
    assert!(!out.contains("warning"), "{out}");
    assert!(out.contains("ok: 4 panel(s)"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn bad_input_fails_with_exit_1_and_writes_nothing() {
    let dir = temp("bad");
    let out = dir.join("out");
    // Limit violation in one panel: no panel may be written.
    let m = std::fs::read_to_string(ex("machine.toml"))
        .unwrap()
        .replace("y = [0, 300]", "y = [40, 300]");
    let mp = dir.join("machine.toml");
    std::fs::write(&mp, m).unwrap();
    let r = bin()
        .args(["gen", "--machine"])
        .arg(&mp)
        .args(["--wing", &ex("wing.toml"), "-o"])
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(r.status.code(), Some(1));
    let err = String::from_utf8_lossy(&r.stderr);
    assert!(
        err.starts_with("error: ") && err.contains("axis limit violation"),
        "{err}"
    );
    assert!(!out.exists(), "nothing may be written when any panel fails");

    // Missing files and broken airfoils name the file.
    let r = bin().args(["info", "/nonexistent/x.dat"]).output().unwrap();
    assert_eq!(r.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&r.stderr).contains("/nonexistent/x.dat"));
    let bad = dir.join("bad.dat");
    std::fs::write(&bad, "junk\n1 0\nfoo bar\n").unwrap();
    let r = bin().arg("info").arg(&bad).output().unwrap();
    assert_eq!(r.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&r.stderr).contains("bad.dat"));

    // Usage errors are clap's exit code 2.
    let r = bin().args(["gen", "--wing", "w.toml"]).output().unwrap();
    assert_eq!(r.status.code(), Some(2));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn prints_shell_completions() {
    for shell in ["bash", "zsh", "fish"] {
        let r = bin().args(["completions", shell]).output().unwrap();
        assert!(r.status.success(), "{shell}");
        let s = String::from_utf8_lossy(&r.stdout);
        assert!(
            s.contains("wingcut") && s.contains("gen") && s.contains("machine"),
            "{shell}"
        );
    }
    let r = bin().args(["completions", "tcsh"]).output().unwrap();
    assert_eq!(r.status.code(), Some(2));
}
