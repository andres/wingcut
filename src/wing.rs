//! Wing model: span stations → interpolated sections in wing coordinates
//! (mm, x aft from the wing's x = 0 reference, y up).

use crate::airfoil::{Airfoil, Profile, points_for};
use crate::config::{Panel, Wing};
use crate::geom::P2;
use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;

pub struct WingModel {
    pub wing: Wing,
    /// Points per surface for every section of this wing.
    pub n: usize,
    /// Resampled, normalised profile of each station.
    station_profiles: Vec<Profile>,
}

/// Planform values at one span position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Planform {
    pub chord: f64,
    pub le_offset: f64,
    pub dihedral_offset: f64,
    pub twist_deg: f64,
    pub twist_pivot: f64,
}

impl WingModel {
    pub fn new(wing: Wing, max_segment: f64) -> Result<WingModel> {
        let max_chord = wing.stations.iter().map(|s| s.chord).fold(0.0, f64::max);
        let n = points_for(max_chord, max_segment);
        let mut cache: HashMap<PathBuf, Profile> = HashMap::new();
        let mut station_profiles = Vec::new();
        for s in &wing.stations {
            let path = wing.base_dir.join(&s.airfoil);
            if !cache.contains_key(&path) {
                let p = Airfoil::load(&path)?.resample(n);
                cache.insert(path.clone(), p);
            }
            station_profiles.push(cache[&path].clone());
        }
        Ok(WingModel {
            wing,
            n,
            station_profiles,
        })
    }

    /// Index `i` of the station interval [i, i+1] holding `span`, and the
    /// fraction within it. Deterministic, so equal spans give equal sections.
    fn locate(&self, span: f64) -> (usize, f64) {
        let st = &self.wing.stations;
        let last = st.len() - 2;
        let i = st[..=last]
            .iter()
            .rposition(|s| s.span <= span)
            .unwrap_or(0);
        let f = ((span - st[i].span) / (st[i + 1].span - st[i].span)).clamp(0.0, 1.0);
        (i, f)
    }

    pub fn planform_at(&self, span: f64) -> Planform {
        let (i, f) = self.locate(span);
        let (a, b) = (&self.wing.stations[i], &self.wing.stations[i + 1]);
        let l = |x: f64, y: f64| {
            if f == 0.0 {
                x
            } else if f == 1.0 {
                y
            } else {
                x + (y - x) * f
            }
        };
        Planform {
            chord: l(a.chord, b.chord),
            le_offset: l(a.le_offset, b.le_offset),
            dihedral_offset: l(a.dihedral_offset, b.dihedral_offset),
            twist_deg: l(a.twist_deg, b.twist_deg),
            twist_pivot: l(a.twist_pivot, b.twist_pivot),
        }
    }

    /// Normalised (chord 1) profile at `span`, blended between stations.
    pub fn profile_at(&self, span: f64) -> Profile {
        let (i, f) = self.locate(span);
        self.station_profiles[i].blend(&self.station_profiles[i + 1], f)
    }

    /// The wing section at `span` in wing coordinates (mm): scaled by chord,
    /// TE thickened to `te_min`, twisted about the pivot, then offset by the
    /// sweep (`le_offset`) and dihedral offsets.
    pub fn section_at(&self, span: f64, te_min: f64) -> Profile {
        let pf = self.planform_at(span);
        let mut p = self.profile_at(span).map(|q| q * pf.chord);
        enforce_te_min(&mut p, te_min, pf.chord);
        // Positive twist = nose up: with x aft and y up that is a clockwise
        // (negative) rotation about the pivot.
        let pivot = P2::new(pf.twist_pivot * pf.chord, 0.0);
        let ang = -pf.twist_deg.to_radians();
        let shift = P2::new(pf.le_offset, pf.dihedral_offset);
        p.map(|q| q.rotate_about(pivot, ang) + shift)
    }

    /// Interior stations strictly inside a panel. A hot wire cuts a ruled
    /// surface, so any planform kink there is lost.
    pub fn stations_inside(&self, panel: &Panel) -> Vec<f64> {
        self.wing
            .stations
            .iter()
            .map(|s| s.span)
            .filter(|&s| s > panel.from + 1e-9 && s < panel.to - 1e-9)
            .collect()
    }
}

/// Thicken the aft half of the section so that upper − lower ≥ `te_min`,
/// symmetrically about the camber line. `p` is chord-scaled, untwisted.
pub fn enforce_te_min(p: &mut Profile, te_min: f64, chord: f64) {
    for (u, l) in p.upper.iter_mut().zip(p.lower.iter_mut()) {
        if u.x < 0.5 * chord {
            continue;
        }
        let t = u.y - l.y;
        if t < te_min {
            let mid = 0.5 * (u.y + l.y);
            u.y = mid + te_min / 2.0;
            l.y = mid - te_min / 2.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Station;

    fn station(span: f64, chord: f64, af: &str) -> Station {
        Station {
            span,
            chord,
            airfoil: PathBuf::from(af),
            le_offset: 0.0,
            dihedral_offset: 0.0,
            twist_deg: 0.0,
            twist_pivot: 0.25,
        }
    }

    pub fn wing(stations: Vec<Station>) -> Wing {
        let mut w = Wing {
            name: "t".into(),
            units: "mm".into(),
            stations,
            panels: vec![],
            base_dir: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples"),
        };
        w.validate().unwrap();
        w
    }

    #[test]
    fn interpolates_planform_linearly() {
        let mut a = station(0.0, 260.0, "airfoils/naca2412.dat");
        let mut b = station(1000.0, 220.0, "airfoils/naca2410.dat");
        a.le_offset = 0.0;
        b.le_offset = 20.0;
        b.twist_deg = -1.0;
        let m = WingModel::new(wing(vec![a, b]), 1.0).unwrap();
        let p = m.planform_at(250.0);
        assert!((p.chord - 250.0).abs() < 1e-12);
        assert!((p.le_offset - 5.0).abs() < 1e-12);
        assert!((p.twist_deg + 0.25).abs() < 1e-12);
    }

    #[test]
    fn twist_is_about_pivot_and_washout_drops_le() {
        let a = station(0.0, 200.0, "airfoils/naca2412.dat");
        let mut b = station(1000.0, 200.0, "airfoils/naca2412.dat");
        b.twist_deg = -2.0;
        let m = WingModel::new(wing(vec![a, b]), 1.0).unwrap();
        let s0 = m.section_at(0.0, 0.0);
        let s1 = m.section_at(1000.0, 0.0);
        // LE drops (washout = nose down), TE rises.
        assert!(s1.upper[0].y < s0.upper[0].y - 1.0);
        let te = s1.upper.len() - 1;
        assert!(s1.upper[te].y > s0.upper[te].y + 1.0);
        // Pivot (25 % chord on the chord line) is fixed: LE and TE move by
        // amounts in the ratio 0.25 : 0.75.
        let dle = s0.upper[0].y - s1.upper[0].y;
        let dte = s1.lower[te].y - s0.lower[te].y;
        assert!((dte / dle - 3.0).abs() < 0.05, "{}", dte / dle);
    }

    #[test]
    fn te_min_thickness_is_enforced() {
        let a = station(0.0, 200.0, "airfoils/naca2412.dat");
        let b = station(1000.0, 200.0, "airfoils/naca2412.dat");
        let m = WingModel::new(wing(vec![a, b]), 1.0).unwrap();
        let s = m.section_at(0.0, 1.5);
        let te = s.upper.len() - 1;
        assert!(s.upper[te].y - s.lower[te].y >= 1.5 - 1e-9);
        // The forward part is untouched.
        let raw = m.section_at(0.0, 0.0);
        assert_eq!(s.upper[5], raw.upper[5]);
    }

    #[test]
    fn same_span_gives_identical_sections() {
        let st = vec![
            station(0.0, 260.0, "airfoils/naca2412.dat"),
            station(1000.0, 220.0, "airfoils/naca2410.dat"),
            station(2000.0, 140.0, "airfoils/naca2412.dat"),
        ];
        let m = WingModel::new(wing(st), 1.0).unwrap();
        assert_eq!(m.section_at(1000.0, 0.8), m.section_at(1000.0, 0.8));
        // At a station the blended profile equals the station's own profile.
        assert_eq!(m.profile_at(1000.0), m.station_profiles[1]);
    }
}
