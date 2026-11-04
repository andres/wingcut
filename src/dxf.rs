//! Minimal ASCII DXF reader for airfoil profiles.
//!
//! Reads `LWPOLYLINE`, `POLYLINE`/`VERTEX`, `LINE` and `SPLINE` fit points
//! from the ENTITIES section, chains them into outlines, takes the largest
//! one and returns its points in airfoil order (TE → one surface → LE →
//! other surface → TE). Arcs, bulges and spline control points are not
//! supported: export the profile as a polyline.

use crate::geom::P2;
use anyhow::{Result, bail};

/// One parsed entity: a run of points and whether it is a closed loop.
struct Chain {
    pts: Vec<P2>,
    closed: bool,
}

/// Parse DXF text into the profile's points in airfoil order.
pub fn profile_points(text: &str) -> Result<Vec<P2>> {
    let pairs = group_pairs(text)?;
    let chains = entities(&pairs)?;
    if chains.is_empty() {
        bail!("no LWPOLYLINE, POLYLINE, LINE or SPLINE entities found");
    }
    let outline = largest(join(chains));
    if outline.pts.len() < 5 {
        bail!("the profile outline has only {} points", outline.pts.len());
    }
    Ok(if outline.closed {
        open_at_te(outline.pts)
    } else {
        outline.pts
    })
}

/// DXF is a list of (group code, value) line pairs.
fn group_pairs(text: &str) -> Result<Vec<(i32, &str)>> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    if !lines.len().is_multiple_of(2) && !lines.last().is_some_and(|l| l.is_empty()) {
        bail!("not an ASCII DXF file (odd number of lines)");
    }
    lines
        .chunks(2)
        .filter(|c| c.len() == 2)
        .enumerate()
        .map(|(i, c)| match c[0].parse::<i32>() {
            Ok(code) => Ok((code, c[1])),
            Err(_) => bail!(
                "not an ASCII DXF file (line {}: expected a group code, got {:?})",
                2 * i + 1,
                c[0]
            ),
        })
        .collect()
}

fn num(v: &str) -> Result<f64> {
    match v.parse::<f64>() {
        Ok(x) if x.is_finite() => Ok(x),
        _ => bail!("bad DXF number {v:?}"),
    }
}

/// Collect point chains from the ENTITIES section.
fn entities(pairs: &[(i32, &str)]) -> Result<Vec<Chain>> {
    // Only look inside the ENTITIES section if there is one.
    let start = pairs
        .windows(2)
        .position(|w| w[0] == (0, "SECTION") && w[1] == (2, "ENTITIES"))
        .map_or(0, |i| i + 2);
    let mut chains = Vec::new();
    let mut i = start;
    let mut polyline: Option<Chain> = None; // open POLYLINE collecting VERTEXes
    while i < pairs.len() {
        let (code, kind) = pairs[i];
        if code != 0 {
            i += 1;
            continue;
        }
        if kind == "ENDSEC" {
            break;
        }
        // Group codes of this entity, up to the next code 0.
        let end = pairs[i + 1..]
            .iter()
            .position(|p| p.0 == 0)
            .map_or(pairs.len(), |k| i + 1 + k);
        let body = &pairs[i + 1..end];
        let xy = |cx: i32, cy: i32| -> Result<Vec<P2>> {
            let mut pts = Vec::new();
            let mut x = None;
            for &(c, v) in body {
                if c == cx {
                    x = Some(num(v)?);
                } else if c == cy
                    && let Some(xv) = x.take()
                {
                    pts.push(P2::new(xv, num(v)?));
                }
            }
            Ok(pts)
        };
        let closed_flag = || {
            body.iter()
                .any(|&(c, v)| c == 70 && v.parse::<i32>().is_ok_and(|f| f & 1 == 1))
        };
        match kind {
            "LWPOLYLINE" => {
                if body
                    .iter()
                    .any(|&(c, v)| c == 42 && num(v).is_ok_and(|b| b != 0.0))
                {
                    bail!(
                        "LWPOLYLINE with arc segments (bulges) is not supported; export straight segments"
                    );
                }
                chains.push(Chain {
                    pts: xy(10, 20)?,
                    closed: closed_flag(),
                });
            }
            "POLYLINE" => {
                polyline = Some(Chain {
                    pts: Vec::new(),
                    closed: closed_flag(),
                })
            }
            "VERTEX" => {
                if let Some(pl) = polyline.as_mut() {
                    pl.pts.extend(xy(10, 20)?);
                }
            }
            "SEQEND" => chains.extend(polyline.take()),
            "LINE" => {
                let (a, b) = (xy(10, 20)?, xy(11, 21)?);
                if let (Some(a), Some(b)) = (a.first(), b.first()) {
                    chains.push(Chain {
                        pts: vec![*a, *b],
                        closed: false,
                    });
                }
            }
            "SPLINE" => {
                let fit = xy(11, 21)?;
                if fit.is_empty() {
                    bail!(
                        "SPLINE without fit points is not supported; export the profile as a polyline"
                    );
                }
                chains.push(Chain {
                    pts: fit,
                    closed: closed_flag(),
                });
            }
            _ => {}
        }
        i = end;
    }
    for c in &mut chains {
        dedupe(&mut c.pts);
        // A polyline whose ends meet is a loop even without the flag.
        if !c.closed && c.pts.len() > 2 && c.pts[0].dist(c.pts[c.pts.len() - 1]) < 1e-9 {
            c.pts.pop();
            c.closed = true;
        }
    }
    chains.retain(|c| c.pts.len() >= 2);
    Ok(chains)
}

fn dedupe(pts: &mut Vec<P2>) {
    pts.dedup_by(|a, b| a.dist(*b) < 1e-12);
}

/// Join open chains end to end (within a small tolerance).
fn join(mut chains: Vec<Chain>) -> Vec<Chain> {
    let size = {
        let all = chains.iter().flat_map(|c| c.pts.iter());
        let (mut lo, mut hi) = (P2::new(f64::MAX, f64::MAX), P2::new(f64::MIN, f64::MIN));
        for p in all {
            lo = P2::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = P2::new(hi.x.max(p.x), hi.y.max(p.y));
        }
        lo.dist(hi)
    };
    let tol = 1e-5 * size.max(1e-9);
    let mut out = Vec::new();
    while let Some(mut cur) = chains.pop() {
        if !cur.closed {
            loop {
                let (head, tail) = (cur.pts[0], cur.pts[cur.pts.len() - 1]);
                let hit = chains.iter().position(|c| {
                    !c.closed && {
                        let (a, b) = (c.pts[0], c.pts[c.pts.len() - 1]);
                        [a, b]
                            .iter()
                            .any(|e| e.dist(tail) < tol || e.dist(head) < tol)
                    }
                });
                let Some(k) = hit else { break };
                let mut c = chains.swap_remove(k).pts;
                let (a, b) = (c[0], c[c.len() - 1]);
                if a.dist(tail) < tol {
                    cur.pts.extend(c.drain(1..));
                } else if b.dist(tail) < tol {
                    c.reverse();
                    cur.pts.extend(c.drain(1..));
                } else if b.dist(head) < tol {
                    c.pop();
                    c.extend(cur.pts);
                    cur.pts = c;
                } else {
                    c.reverse();
                    c.pop();
                    c.extend(cur.pts);
                    cur.pts = c;
                }
            }
            if cur.pts.len() > 2 && cur.pts[0].dist(cur.pts[cur.pts.len() - 1]) < tol {
                cur.pts.pop();
                cur.closed = true;
            }
        }
        out.push(cur);
    }
    out
}

/// The outline with the largest extent (ignores frames' small companions,
/// text, construction lines...).
fn largest(chains: Vec<Chain>) -> Chain {
    let extent = |c: &Chain| {
        let xs = c.pts.iter().map(|p| p.x);
        let (lo, hi) = xs.fold((f64::MAX, f64::MIN), |(a, b), x| (a.min(x), b.max(x)));
        (hi - lo) * c.pts.len().min(4) as f64 // a 2-point line never wins over a profile
    };
    chains
        .into_iter()
        .max_by(|a, b| extent(a).total_cmp(&extent(b)))
        .expect("at least one chain")
}

/// Turn a closed loop into airfoil order starting and ending at the TE.
fn open_at_te(pts: Vec<P2>) -> Vec<P2> {
    let n = pts.len();
    // LE and TE are the two farthest-apart points; the TE is the sharper one.
    let (mut a, mut b, mut best) = (0, 0, -1.0);
    for i in 0..n {
        for j in i + 1..n {
            let d = pts[i].dist(pts[j]);
            if d > best {
                (a, b, best) = (i, j, d);
            }
        }
    }
    let angle_at = |i: usize| -> f64 {
        // Neighbours ~3 % of the chord away along the loop, for robustness.
        let reach = 0.03 * best;
        let walk = |step: isize| -> P2 {
            let mut k = i;
            let mut len = 0.0;
            while len < reach {
                let next = (k as isize + step).rem_euclid(n as isize) as usize;
                len += pts[k].dist(pts[next]);
                k = next;
                if k == i {
                    break;
                }
            }
            pts[k]
        };
        let (u, v) = (walk(-1) - pts[i], walk(1) - pts[i]);
        (u.dot(v) / (u.len() * v.len())).clamp(-1.0, 1.0).acos()
    };
    let (le, te) = if angle_at(a) < angle_at(b) {
        (b, a)
    } else {
        (a, b)
    };
    // Open TE: the short segment at the TE corner running across the chord
    // (the surfaces next to the TE run almost along it) is the gap.
    let chord = pts[te] - pts[le];
    let next = (te + 1) % n;
    let prev = (te + n - 1) % n;
    let crosses = |i: usize, j: usize| {
        let d = pts[j] - pts[i];
        let len = d.len();
        len > 0.0 && len < 0.1 * best && (d.dot(chord) / (len * chord.len())).abs() < 0.5
    };
    let rotate =
        |start: usize, len: usize| -> Vec<P2> { (0..len).map(|k| pts[(start + k) % n]).collect() };
    if crosses(te, next) {
        rotate(next, n) // next … te
    } else if crosses(prev, te) {
        rotate(te, n) // te … prev
    } else {
        let mut v = rotate(te, n);
        v.push(pts[te]); // sharp TE: start and end at the same point
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::airfoil::{Airfoil, tests::naca4, tests::selig_text};

    fn loop_pts() -> Vec<P2> {
        let (u, l) = naca4(0.02, 0.4, 0.12, 121);
        // Open TE (NACA formula closes it; open it a little), scaled to mm.
        let mut v: Vec<P2> = u.iter().rev().chain(l.iter().skip(1)).copied().collect();
        let last = v.len() - 1;
        v[0].y += 0.002;
        v[last].y -= 0.002;
        v.iter().map(|p| *p * 180.0 + P2::new(20.0, 5.0)).collect()
    }

    fn lwpolyline(pts: &[P2], closed: bool) -> String {
        let mut s = String::from("0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n8\n0\n");
        s += &format!("90\n{}\n70\n{}\n", pts.len(), u8::from(closed));
        for p in pts {
            s += &format!("10\n{}\n20\n{}\n", p.x, p.y);
        }
        s + "0\nENDSEC\n0\nEOF\n"
    }

    fn reference() -> crate::airfoil::Profile {
        let v = loop_pts();
        let up: Vec<P2> = v[..61].iter().rev().copied().collect();
        let lo: Vec<P2> = v[60..].to_vec();
        Airfoil::parse(&selig_text(&up, &lo)).unwrap().resample(101)
    }

    fn assert_same(text: &str) {
        let pts = profile_points(text).unwrap();
        let af = Airfoil::from_dxf_points("t".into(), pts).unwrap();
        let (a, b) = (af.resample(101), reference());
        for (p, q) in a.cut_order().iter().zip(b.cut_order()) {
            assert!(p.dist(q) < 1e-6, "{p:?} vs {q:?}");
        }
    }

    #[test]
    fn open_lwpolyline_in_selig_order() {
        assert_same(&lwpolyline(&loop_pts(), false));
    }

    #[test]
    fn closed_lwpolyline_starting_anywhere_either_direction() {
        let v = loop_pts();
        for start in [0, 17, 60, 100] {
            let mut r: Vec<P2> = v[start..].iter().chain(&v[..start]).copied().collect();
            assert_same(&lwpolyline(&r, true));
            r.reverse();
            assert_same(&lwpolyline(&r, true));
        }
    }

    #[test]
    fn closed_loop_with_sharp_te() {
        let (u, l) = naca4(0.02, 0.4, 0.12, 121); // closed TE
        let v: Vec<P2> = u
            .iter()
            .rev()
            .chain(l.iter().skip(1))
            .map(|p| *p * 150.0)
            .collect();
        let r: Vec<P2> = v[30..v.len() - 1].iter().chain(&v[..30]).copied().collect();
        let pts = profile_points(&lwpolyline(&r, true)).unwrap();
        assert_eq!(pts[0], pts[pts.len() - 1], "starts and ends at the TE");
        let af = Airfoil::from_dxf_points("t".into(), pts).unwrap();
        assert!(!af.is_open_te());
        let want = Airfoil::parse(&selig_text(&u, &l)).unwrap().resample(81);
        for (i, (p, q)) in af
            .resample(81)
            .cut_order()
            .iter()
            .zip(want.cut_order())
            .enumerate()
        {
            // The reference went through 6-decimal text; the DXF did not.
            assert!(p.dist(q) < 1e-5, "{i}: {p:?} vs {q:?}");
        }
    }

    #[test]
    fn scattered_lines_are_chained() {
        let v = loop_pts();
        let mut s = String::from("0\nSECTION\n2\nENTITIES\n");
        // Lines in shuffled order and random directions, plus a stray TEXT.
        let mut idx: Vec<usize> = (0..v.len() - 1).collect();
        idx.reverse();
        idx.swap(3, 40);
        for (k, i) in idx.into_iter().enumerate() {
            let (a, b) = if k % 2 == 0 {
                (v[i], v[i + 1])
            } else {
                (v[i + 1], v[i])
            };
            s += &format!(
                "0\nLINE\n8\n0\n10\n{}\n20\n{}\n11\n{}\n21\n{}\n",
                a.x, a.y, b.x, b.y
            );
        }
        s += "0\nTEXT\n10\n0\n20\n0\n1\nhello\n0\nENDSEC\n0\nEOF\n";
        assert_same(&s);
    }

    #[test]
    fn polyline_with_vertices() {
        let mut s = String::from("0\nSECTION\n2\nENTITIES\n0\nPOLYLINE\n8\n0\n70\n0\n");
        for p in loop_pts() {
            s += &format!("0\nVERTEX\n8\n0\n10\n{}\n20\n{}\n", p.x, p.y);
        }
        s += "0\nSEQEND\n0\nENDSEC\n0\nEOF\n";
        assert_same(&s);
    }

    #[test]
    fn rejects_what_it_cannot_read() {
        assert!(
            profile_points("hello\nworld\nfoo\n")
                .unwrap_err()
                .to_string()
                .contains("not an ASCII DXF")
        );
        let e =
            profile_points("0\nSECTION\n2\nENTITIES\n0\nCIRCLE\n10\n0\n20\n0\n40\n5\n0\nENDSEC\n")
                .unwrap_err();
        assert!(e.to_string().contains("no LWPOLYLINE"));
        let e = profile_points("0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n10\n0\n20\n0\n42\n0.5\n10\n1\n20\n0\n0\nENDSEC\n").unwrap_err();
        assert!(e.to_string().contains("bulges"));
    }
}
