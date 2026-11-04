//! SVG preview: both face profiles (nominal and with kerf), both tower
//! paths, the block outline and the axis limits, in machine coordinates.

use crate::config::{Machine, Panel};
use crate::cut::CutPlan;
use crate::geom::P2;
use std::fmt::Write;

pub fn emit(plan: &CutPlan, m: &Machine, panel: &Panel) -> String {
    let b = &panel.block;
    // Frame the block and the wire paths; the axis limits are drawn too but
    // may fall outside the view when they are far away.
    let mut pts: Vec<P2> = plan.root_tower.clone();
    pts.extend(&plan.tip_tower);
    pts.push(P2::new(b.x0, b.y0));
    pts.push(P2::new(b.x0 + b.width, b.y0 + b.height));
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in &pts {
        (x0, y0, x1, y1) = (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y));
    }
    let pad = 0.04 * (x1 - x0).max(y1 - y0);
    let (vx, vy, vw, vh) = (
        x0 - pad,
        -(y1 + pad),
        x1 - x0 + 2.0 * pad,
        y1 - y0 + 2.0 * pad,
    );
    let fs = vw / 70.0;
    let legend_h = fs * 8.5;
    let px_w = 1400.0;
    let px_h = px_w * (vh + legend_h) / vw;

    let mut s = String::new();
    let w = &mut s;
    let _ = writeln!(
        w,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{px_w:.0}" height="{px_h:.0}" viewBox="{vx:.3} {vy:.3} {vw:.3} {:.3}" font-family="sans-serif">"#,
        vh + legend_h
    );
    let _ = writeln!(
        w,
        r#"<rect x="{vx:.3}" y="{vy:.3}" width="{vw:.3}" height="{:.3}" fill="white"/>"#,
        vh + legend_h
    );
    let _ = writeln!(
        w,
        r#"<clipPath id="view"><rect x="{vx:.3}" y="{:.3}" width="{vw:.3}" height="{vh:.3}"/></clipPath>"#,
        -(vy + vh)
    );
    let _ = writeln!(
        w,
        r#"<g transform="scale(1,-1)" clip-path="url(#view)" fill="none" stroke-linejoin="round">"#
    );
    let rect = |w: &mut String, xa: f64, ya: f64, xb: f64, yb: f64, style: &str| {
        let _ = writeln!(
            w,
            r#"<rect x="{xa:.3}" y="{ya:.3}" width="{:.3}" height="{:.3}" {style} vector-effect="non-scaling-stroke"/>"#,
            xb - xa,
            yb - ya
        );
    };
    rect(
        w,
        m.limits.x[0],
        m.limits.y[0],
        m.limits.x[1],
        m.limits.y[1],
        r##"stroke="#999" stroke-dasharray="6 4""##,
    );
    rect(
        w,
        b.x0,
        b.y0,
        b.x0 + b.width,
        b.y0 + b.height,
        r##"fill="#fff4cc" stroke="#c9a400""##,
    );
    let line = |w: &mut String, v: &[P2], color: &str, width: f64, dash: &str| {
        let _ = write!(
            w,
            r#"<polyline stroke="{color}" stroke-width="{width}" {dash} vector-effect="non-scaling-stroke" points=""#
        );
        for p in v {
            let _ = write!(w, "{:.3},{:.3} ", p.x, p.y);
        }
        let _ = writeln!(w, r#""/>"#);
    };
    let [f0, f1] = &plan.faces;
    line(w, &plan.root_tower, "#2a9d4b", 1.0, "");
    line(w, &plan.tip_tower, "#e07b00", 1.0, "");
    line(w, &f0.nominal, "#1f4fd1", 1.0, r#"stroke-dasharray="3 3""#);
    line(w, &f1.nominal, "#d12f2f", 1.0, r#"stroke-dasharray="3 3""#);
    line(w, &f0.path, "#1f4fd1", 1.6, "");
    line(w, &f1.path, "#d12f2f", 1.6, "");
    let _ = writeln!(w, "</g>");

    // Legend below the drawing (unflipped coordinates).
    let ly = vy + vh + fs * 1.3;
    let role = |f: &crate::cut::Face| {
        if f.is_wing_root {
            "wing root"
        } else {
            "wing tip"
        }
    };
    let items = [
        (
            "#1f4fd1".to_string(),
            format!(
                "face s={:.0} ({}), kerf {:.2} mm; dashed = nominal",
                f0.s,
                role(f0),
                f0.kerf
            ),
        ),
        (
            "#d12f2f".to_string(),
            format!(
                "face s={:.0} ({}), kerf {:.2} mm; dashed = nominal",
                f1.s,
                role(f1),
                f1.kerf
            ),
        ),
        (
            "#2a9d4b".to_string(),
            format!("root tower path ({}/{})", m.axes[0], m.axes[1]),
        ),
        (
            "#e07b00".to_string(),
            format!("tip tower path ({}/{})", m.axes[2], m.axes[3]),
        ),
        (
            "#999".to_string(),
            format!(
                "axis limits x {:?} y {:?} (dashed); yellow = foam block",
                m.limits.x, m.limits.y
            ),
        ),
    ];
    let _ = writeln!(
        w,
        r#"<text x="{:.3}" y="{ly:.3}" font-size="{fs:.3}" font-weight="bold">{}</text>"#,
        vx + pad,
        xml(&plan.panel)
    );
    for (i, (c, t)) in items.iter().enumerate() {
        let y = ly + fs * 1.3 * (i as f64 + 1.0);
        let x = vx + pad;
        let _ = writeln!(
            w,
            r#"<rect x="{x:.3}" y="{:.3}" width="{:.3}" height="{:.3}" fill="{c}"/><text x="{:.3}" y="{y:.3}" font-size="{:.3}">{}</text>"#,
            y - fs * 0.6,
            fs * 1.5,
            fs * 0.6,
            x + fs * 2.0,
            fs,
            xml(t)
        );
    }
    let _ = writeln!(w, "</svg>");
    s
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
