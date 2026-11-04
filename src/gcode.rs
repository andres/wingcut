//! G-code output (grbl / FluidNC dialect, 4 linear axes, G93 while cutting).

use crate::config::Machine;
use crate::cut::{CutPlan, Move};
use std::fmt::Write;

/// Fixed 3-decimal formatting without scientific notation or "-0.000".
pub fn num(v: f64) -> String {
    let s = format!("{v:.3}");
    if s == "-0.000" { "0.000".into() } else { s }
}

pub fn fmt_minutes(min: f64) -> String {
    let s = (min * 60.0).round() as u64;
    format!("{} min {:02} s", s / 60, s % 60)
}

pub fn emit(plan: &CutPlan, m: &Machine, wing_name: &str) -> String {
    let ax: Vec<String> = m.axes.iter().map(|a| a.to_ascii_uppercase()).collect();
    let mut g = String::new();
    let w = &mut g;
    let [f0, f1] = &plan.faces;
    let role = |f: &crate::cut::Face| {
        if f.is_wing_root {
            "wing root"
        } else {
            "wing tip"
        }
    };
    // Header (ASCII only; some senders choke on anything else).
    let _ = writeln!(
        w,
        "; wingcut {} - hot-wire G-code",
        env!("CARGO_PKG_VERSION")
    );
    let _ = writeln!(w, "; wing: {wing_name}  panel: {}", plan.panel);
    let _ = writeln!(
        w,
        "; machine: tower spacing {} mm, axes {} (root x/y, tip x/y), limits x {:?} y {:?}",
        m.tower_spacing,
        ax.join(" "),
        m.limits.x,
        m.limits.y
    );
    let _ = writeln!(
        w,
        "; feed {} mm/min at the faster face, rapid {} mm/min, heat '{}' / '{}'",
        m.feed_cut, m.feed_rapid, m.heat_on, m.heat_off
    );
    for f in [f0, f1] {
        let _ = writeln!(
            w,
            "; face s={} ({}): kerf {:.2} mm at avg {:.0} mm/min (min {:.0}, max {:.0})",
            num(f.s),
            role(f),
            f.kerf,
            f.speed_avg,
            f.speed_min,
            f.speed_max
        );
    }
    let _ = writeln!(
        w,
        "; estimated time: {} (cutting {})",
        fmt_minutes(plan.total_minutes),
        fmt_minutes(plan.cut_minutes)
    );
    let _ = writeln!(
        w,
        "; start with the wire parked in front of the block (x below the block front), wire cold"
    );
    let _ = writeln!(w, "G21 G90 G94");
    let _ = writeln!(w, "{}", m.heat_off);
    let mut in_g93 = false;
    for mv in &plan.moves {
        match mv {
            Move::Cut { .. } if !in_g93 => {
                let _ = writeln!(w, "G93");
                in_g93 = true;
            }
            Move::RapidX { .. } | Move::RapidY { .. } if in_g93 => {
                let _ = writeln!(w, "G94");
                in_g93 = false;
            }
            _ => {}
        }
        let rapid = num(m.feed_rapid);
        let _ = match mv {
            Move::RapidY { root, tip } => writeln!(
                w,
                "G1 {}{} {}{} F{rapid}",
                ax[1],
                num(*root),
                ax[3],
                num(*tip)
            ),
            Move::RapidX { root, tip } => writeln!(
                w,
                "G1 {}{} {}{} F{rapid}",
                ax[0],
                num(*root),
                ax[2],
                num(*tip)
            ),
            Move::HeatOn => writeln!(w, "{}", m.heat_on),
            Move::HeatOff => writeln!(w, "{}", m.heat_off),
            Move::Dwell { seconds, note } => writeln!(w, "G4 P{} ; {note}", num(*seconds)),
            Move::Cut { root, tip, minutes } => writeln!(
                w,
                "G1 {}{} {}{} {}{} {}{} F{}",
                ax[0],
                num(root.x),
                ax[1],
                num(root.y),
                ax[2],
                num(tip.x),
                ax[3],
                num(tip.y),
                num(1.0 / minutes)
            ),
        };
    }
    if in_g93 {
        let _ = writeln!(w, "G94");
    }
    let _ = writeln!(w, "{}", m.heat_off);
    let _ = writeln!(w, "M2");
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_have_three_decimals_and_no_exponent() {
        assert_eq!(num(1e-9), "0.000");
        assert_eq!(num(-1e-9), "0.000");
        assert_eq!(num(1234567.891234), "1234567.891");
        assert_eq!(num(-2.5), "-2.500");
        assert!(!num(1e20).contains('e'));
    }

    #[test]
    fn minutes_format() {
        assert_eq!(fmt_minutes(1.5), "1 min 30 s");
    }
}
