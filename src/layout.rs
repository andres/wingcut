//! Laying out panels: split a wing into blocks the machine can cut.

use crate::config::{Panel, Side, Wing};

/// How to split the half-span into panels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Split {
    /// Break at every station; intervals longer than the maximum are
    /// divided into equal parts. Recommended: no station falls inside a
    /// panel, so every planform kink is cut.
    AtStations,
    /// This many equal panels per side (stations may fall inside panels).
    Equal(usize),
}

/// Which wing halves to create panels for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Sides {
    Both,
    Left,
    Right,
}

/// Span intervals (from, to) covering the wing from its first to its last
/// station, none longer than `max_len`.
pub fn intervals(wing: &Wing, split: Split, max_len: f64) -> Vec<(f64, f64)> {
    let spans: Vec<f64> = wing.stations.iter().map(|s| s.span).collect();
    let (Some(&first), Some(&last)) = (spans.first(), spans.last()) else {
        return Vec::new();
    };
    let mut cuts = match split {
        Split::AtStations => spans.clone(),
        Split::Equal(n) => {
            let n = n.max(1);
            (0..=n)
                .map(|i| first + (last - first) * i as f64 / n as f64)
                .collect()
        }
    };
    cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    let mut out = Vec::new();
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let pieces = if max_len > 0.0 {
            ((b - a) / max_len - 1e-9).ceil().max(1.0) as usize
        } else {
            1
        };
        for k in 0..pieces {
            let f0 = a + (b - a) * k as f64 / pieces as f64;
            let f1 = if k + 1 == pieces {
                b
            } else {
                a + (b - a) * (k + 1) as f64 / pieces as f64
            };
            out.push((f0, f1));
        }
    }
    out
}

/// Panels for `intervals`, named `p1-L`, `p1-R`, ... from the root outward.
/// Block settings and sheeting come from `template`; each block is centred
/// between the towers. Cutouts are not copied (they are panel-specific).
pub fn make_panels(
    spans: &[(f64, f64)],
    sides: Sides,
    template: &Panel,
    tower_spacing: f64,
) -> Vec<Panel> {
    let mut out = Vec::new();
    for (i, &(from, to)) in spans.iter().enumerate() {
        let list: &[(Side, &str)] = match sides {
            Sides::Both => &[(Side::Left, "L"), (Side::Right, "R")],
            Sides::Left => &[(Side::Left, "L")],
            Sides::Right => &[(Side::Right, "R")],
        };
        for &(side, tag) in list {
            let mut p = template.clone();
            p.name = format!("p{}-{tag}", i + 1);
            p.from = from;
            p.to = to;
            p.side = side;
            p.cutouts.clear();
            p.wire_current = template.wire_current;
            p.block.length = None;
            p.block.s_root = ((tower_spacing - (to - from)) / 2.0).max(0.0);
            out.push(p);
        }
    }
    out
}

/// Problems with a panel that can be seen without planning it.
pub fn panel_issues(wing: &Wing, p: &Panel, tower_spacing: f64) -> Vec<String> {
    let mut v = Vec::new();
    let len = p.to - p.from;
    if len <= 0.0 {
        v.push("'to' must be larger than 'from'".into());
        return v;
    }
    if len > tower_spacing {
        v.push(format!(
            "{len:.0} mm long: longer than the tower spacing ({tower_spacing:.0} mm)"
        ));
    } else if p.block.s_root < 0.0 || p.block.s_root + len > tower_spacing + 1e-9 {
        v.push(format!(
            "block runs from s = {:.0} to {:.0}, outside the towers (0 to {tower_spacing:.0})",
            p.block.s_root,
            p.block.s_root + len
        ));
    }
    for s in &wing.stations {
        if s.span > p.from + 1e-9 && s.span < p.to - 1e-9 {
            v.push(format!(
                "station at {:.0} mm is inside the panel (its kink is not cut)",
                s.span
            ));
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Block, Sheeting, Station};
    use std::path::PathBuf;

    fn wing(spans: &[f64]) -> Wing {
        Wing {
            name: "w".into(),
            units: "mm".into(),
            stations: spans
                .iter()
                .map(|&span| Station {
                    span,
                    chord: 200.0,
                    airfoil: PathBuf::from("a.dat"),
                    le_offset: 0.0,
                    dihedral_offset: 0.0,
                    twist_deg: 0.0,
                    twist_pivot: 0.25,
                })
                .collect(),
            panels: vec![],
            base_dir: PathBuf::new(),
        }
    }

    fn template() -> Panel {
        Panel {
            name: "t".into(),
            from: 0.0,
            to: 1.0,
            side: Side::Left,
            block: Block {
                s_root: 0.0,
                length: Some(1.0),
                x0: 100.0,
                y0: 20.0,
                height: 60.0,
                width: 300.0,
                inset: 10.0,
                chord_y: None,
            },
            wire_current: None,
            sheeting: Sheeting {
                top: 0.6,
                ..Sheeting::default()
            },
            cutouts: vec![],
        }
    }

    #[test]
    fn splits_at_stations_and_divides_long_intervals() {
        // A 5 m wing: 2500 mm half-span, stations at 0, 1500, 2500.
        let w = wing(&[0.0, 1500.0, 2500.0]);
        let iv = intervals(&w, Split::AtStations, 1250.0);
        assert_eq!(iv, vec![(0.0, 750.0), (750.0, 1500.0), (1500.0, 2500.0)]);
        assert!(iv.iter().all(|(a, b)| b - a <= 1250.0));
    }

    #[test]
    fn equal_split() {
        let w = wing(&[0.0, 2500.0]);
        assert_eq!(
            intervals(&w, Split::Equal(2), 1250.0),
            vec![(0.0, 1250.0), (1250.0, 2500.0)]
        );
        // Still capped at the maximum length.
        assert_eq!(intervals(&w, Split::Equal(1), 1250.0).len(), 2);
    }

    #[test]
    fn makes_centred_mirrored_panels_from_a_template() {
        let ps = make_panels(
            &[(0.0, 1250.0), (1250.0, 2500.0)],
            Sides::Both,
            &template(),
            1600.0,
        );
        let names: Vec<&str> = ps.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["p1-L", "p1-R", "p2-L", "p2-R"]);
        assert!(ps.iter().all(|p| (p.block.s_root - 175.0).abs() < 1e-9));
        assert!(
            ps.iter()
                .all(|p| p.sheeting.top == 0.6 && p.block.length.is_none())
        );
        assert_eq!(ps[1].side, Side::Right);
    }

    #[test]
    fn reports_panel_issues() {
        let w = wing(&[0.0, 1000.0, 2000.0]);
        let mut p = template();
        p.from = 0.0;
        p.to = 2000.0;
        let v = panel_issues(&w, &p, 1600.0);
        assert!(
            v.iter()
                .any(|m| m.contains("longer than the tower spacing"))
        );
        assert!(v.iter().any(|m| m.contains("station at 1000")));
        p.to = 1000.0;
        p.block.s_root = 900.0;
        assert!(panel_issues(&w, &p, 1600.0)[0].contains("outside the towers"));
        p.block.s_root = 300.0;
        assert!(panel_issues(&w, &p, 1600.0).is_empty());
    }
}
