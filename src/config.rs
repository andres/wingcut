//! `machine.toml` and `wing.toml` definitions.

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Machine {
    /// Wire-contact to wire-contact distance between the towers (mm).
    #[serde(default = "d_spacing")]
    pub tower_spacing: f64,
    /// Axis letters: root-x, root-y, tip-x, tip-y.
    #[serde(default = "d_axes")]
    pub axes: [String; 4],
    #[serde(default)]
    pub limits: Limits,
    /// Cutting speed at the faster block face (mm/min).
    #[serde(default = "d_feed_cut")]
    pub feed_cut: f64,
    /// Wire-off positioning speed (mm/min).
    #[serde(default = "d_feed_rapid")]
    pub feed_rapid: f64,
    #[serde(default = "d_heat_on")]
    pub heat_on: String,
    #[serde(default = "d_heat_off")]
    pub heat_off: String,
    /// Dwell after switching the wire on (s).
    #[serde(default = "d_preheat")]
    pub preheat_s: f64,
    /// Optional dwell at the leading edge (s).
    #[serde(default)]
    pub le_dwell_s: f64,
    /// Kerf (mm) vs. face speed (mm/min), from test cuts. Interpolated.
    #[serde(default = "d_kerf")]
    pub kerf: Vec<KerfPoint>,
    /// Minimum trailing-edge thickness (mm).
    #[serde(default = "d_te_min")]
    pub te_min_thickness: f64,
    /// Max profile segment length at the largest chord (mm).
    #[serde(default = "d_max_seg")]
    pub max_segment: f64,
    /// Distance outside the block front where the wire starts and ends (mm).
    #[serde(default = "d_clearance")]
    pub lead_clearance: f64,
    /// Tower x to return to after the cut (wire off, at the exit height).
    #[serde(default)]
    pub park_x: f64,
    /// Miter limit for the kerf offset, as a multiple of kerf/2.
    #[serde(default = "d_miter")]
    pub miter_limit: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub x: [f64; 2],
    pub y: [f64; 2],
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            x: [0.0, 1000.0],
            y: [0.0, 300.0],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct KerfPoint {
    pub feed: f64,
    pub kerf: f64,
}

fn d_spacing() -> f64 {
    1600.0
}
fn d_axes() -> [String; 4] {
    ["X", "Y", "Z", "A"].map(String::from)
}
fn d_feed_cut() -> f64 {
    180.0
}
fn d_feed_rapid() -> f64 {
    800.0
}
fn d_heat_on() -> String {
    "M3 S1000".into()
}
fn d_heat_off() -> String {
    "M5".into()
}
fn d_preheat() -> f64 {
    2.0
}
fn d_kerf() -> Vec<KerfPoint> {
    vec![KerfPoint {
        feed: 180.0,
        kerf: 0.8,
    }]
}
fn d_te_min() -> f64 {
    0.8
}
fn d_max_seg() -> f64 {
    0.2
}
fn d_clearance() -> f64 {
    5.0
}
fn d_miter() -> f64 {
    4.0
}

impl Machine {
    pub fn load(path: &Path) -> Result<Machine> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading machine config {}", path.display()))?;
        Machine::parse(&text).with_context(|| format!("machine config {}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Machine> {
        toml::from_str::<Machine>(text)?.normalized()
    }

    /// Sort the kerf table and validate. Use this on edited copies.
    pub fn normalized(mut self) -> Result<Machine> {
        self.kerf.sort_by(|a, b| a.feed.total_cmp(&b.feed));
        self.validate()?;
        Ok(self)
    }

    pub fn to_toml(&self) -> Result<String> {
        Ok(toml::to_string_pretty(self)?)
    }

    pub fn validate(&self) -> Result<()> {
        // TOML accepts nan/inf; reject them before any comparison can be fooled.
        for (name, v) in [
            ("tower_spacing", self.tower_spacing),
            ("feed_cut", self.feed_cut),
            ("feed_rapid", self.feed_rapid),
            ("preheat_s", self.preheat_s),
            ("le_dwell_s", self.le_dwell_s),
            ("te_min_thickness", self.te_min_thickness),
            ("max_segment", self.max_segment),
            ("lead_clearance", self.lead_clearance),
            ("park_x", self.park_x),
            ("miter_limit", self.miter_limit),
            ("limits.x[0]", self.limits.x[0]),
            ("limits.x[1]", self.limits.x[1]),
            ("limits.y[0]", self.limits.y[0]),
            ("limits.y[1]", self.limits.y[1]),
        ] {
            finite(name, v)?;
        }
        for k in &self.kerf {
            finite("kerf.feed", k.feed)?;
            finite("kerf.kerf", k.kerf)?;
        }
        for (name, cmd) in [("heat_on", &self.heat_on), ("heat_off", &self.heat_off)] {
            ensure!(
                !cmd.trim().is_empty()
                    && cmd.chars().all(|c| c.is_ascii() && !c.is_ascii_control()),
                "{name} must be one non-empty line of plain ASCII, got {cmd:?}"
            );
        }
        ensure!(self.tower_spacing > 0.0, "tower_spacing must be > 0");
        ensure!(self.feed_cut > 0.0, "feed_cut must be > 0");
        ensure!(self.feed_rapid > 0.0, "feed_rapid must be > 0");
        ensure!(self.max_segment > 0.0, "max_segment must be > 0");
        ensure!(
            self.preheat_s >= 0.0 && self.le_dwell_s >= 0.0,
            "dwells must be >= 0"
        );
        ensure!(
            self.te_min_thickness >= 0.0,
            "te_min_thickness must be >= 0"
        );
        ensure!(self.miter_limit >= 1.0, "miter_limit must be >= 1");
        for (i, a) in self.axes.iter().enumerate() {
            ensure!(
                a.len() == 1 && a.chars().all(|c| c.is_ascii_alphabetic()),
                "axis {i} must be a single letter, got {a:?}"
            );
            let up = a.to_ascii_uppercase();
            ensure!(
                !matches!(up.as_str(), "F" | "G" | "M" | "N" | "P" | "S" | "T"),
                "axis letter {a:?} clashes with a G-code word"
            );
            for b in &self.axes[..i] {
                ensure!(!b.eq_ignore_ascii_case(a), "axis letter {a:?} used twice");
            }
        }
        for l in [self.limits.x, self.limits.y] {
            ensure!(l[0] < l[1], "limits must be [min, max] with min < max");
        }
        ensure!(!self.kerf.is_empty(), "kerf table is empty");
        for k in &self.kerf {
            ensure!(k.feed > 0.0 && k.kerf >= 0.0, "bad kerf entry {k:?}");
        }
        Ok(())
    }

    pub fn kerf_at(&self, feed: f64) -> f64 {
        let t: Vec<(f64, f64)> = self.kerf.iter().map(|k| (k.feed, k.kerf)).collect();
        crate::geom::interp_table(&t, feed)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Wing {
    pub name: String,
    #[serde(default = "d_units")]
    pub units: String,
    #[serde(rename = "station")]
    pub stations: Vec<Station>,
    #[serde(rename = "panel", default)]
    pub panels: Vec<Panel>,
    /// Directory of the wing file; airfoil paths are relative to it.
    #[serde(skip)]
    pub base_dir: PathBuf,
}

fn finite(name: &str, v: f64) -> Result<()> {
    ensure!(v.is_finite(), "{name} must be a finite number, got {v}");
    Ok(())
}

fn d_units() -> String {
    "mm".into()
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Station {
    /// Distance along the half-span from the centreline (mm).
    pub span: f64,
    pub chord: f64,
    pub airfoil: PathBuf,
    /// Sweep: x offset of the LE, positive = aft (mm).
    #[serde(default)]
    pub le_offset: f64,
    /// Dihedral/polyhedral: y offset (mm).
    #[serde(default)]
    pub dihedral_offset: f64,
    /// Twist (deg), positive = nose up; washout is negative.
    #[serde(default)]
    pub twist_deg: f64,
    /// Twist pivot as a fraction of chord.
    #[serde(default = "d_pivot")]
    pub twist_pivot: f64,
}

fn d_pivot() -> f64 {
    0.25
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    /// Wing root on the root-tower side of the block.
    Left,
    /// Mirrored: wing root on the tip-tower side.
    Right,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Panel {
    pub name: String,
    pub from: f64,
    pub to: f64,
    #[serde(default = "d_side")]
    pub side: Side,
    pub block: Block,
    /// Spar notches and hinge cuts, cut as slots into the section.
    #[serde(rename = "cutout", default, skip_serializing_if = "Vec::is_empty")]
    pub cutouts: Vec<Cutout>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Surface {
    Upper,
    Lower,
}

/// A rectangular slot cut into one surface, with walls perpendicular to the
/// chord line. Same chord fraction on both faces; width and depth in mm, so
/// a constant-section spar fits along the whole panel.
///
/// * Spar notch: `depth` = slot depth below the surface at its centre.
/// * Hinge cut: `leave` = material left at the opposite surface (the slot
///   goes almost through, e.g. `leave = 0.5`).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Cutout {
    pub surface: Surface,
    /// Chord fraction of the slot centre (0 = LE, 1 = TE).
    pub x: f64,
    /// Slot width along the chord (mm).
    pub width: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leave: Option<f64>,
}

fn d_side() -> Side {
    Side::Left
}

/// Foam block placement on the machine. All values in mm.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Block {
    /// Span position of the block face nearest the root tower.
    pub s_root: f64,
    /// Block length along the span. Optional; if given it must equal the
    /// panel span (`to - from`), because the block faces are the cut faces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<f64>,
    /// Machine x of the block front face (the side the wire enters).
    pub x0: f64,
    /// Machine y of the block bottom.
    pub y0: f64,
    pub height: f64,
    /// Block depth along x (only used for the preview and fit warnings).
    #[serde(default = "d_width")]
    pub width: f64,
    /// Gap from the block front to the rearmost trailing edge (mm).
    #[serde(default = "d_inset")]
    pub inset: f64,
    /// Height of the wing reference line (y = 0) above the block bottom.
    /// Default: centre the panel vertically in the block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chord_y: Option<f64>,
}

fn d_width() -> f64 {
    600.0
}
fn d_inset() -> f64 {
    10.0
}

impl Wing {
    pub fn load(path: &Path) -> Result<Wing> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading wing config {}", path.display()))?;
        let base = path.parent().unwrap_or(Path::new("."));
        Wing::parse(&text, base).with_context(|| format!("wing config {}", path.display()))
    }

    /// Parse a wing; airfoil paths are resolved relative to `base_dir`.
    pub fn parse(text: &str, base_dir: &Path) -> Result<Wing> {
        let mut w: Wing = toml::from_str(text)?;
        w.base_dir = base_dir.to_path_buf();
        w.validate()?;
        Ok(w)
    }

    pub fn validate(&mut self) -> Result<()> {
        ensure!(self.units == "mm", "only units = \"mm\" is supported");
        ensure!(self.stations.len() >= 2, "need at least two [[station]]s");
        for st in &self.stations {
            for (name, v) in [
                ("span", st.span),
                ("chord", st.chord),
                ("le_offset", st.le_offset),
                ("dihedral_offset", st.dihedral_offset),
                ("twist_deg", st.twist_deg),
                ("twist_pivot", st.twist_pivot),
            ] {
                finite(&format!("station {name}"), v)?;
            }
        }
        for p in &self.panels {
            let b = &p.block;
            for (name, v) in [
                ("from", p.from),
                ("to", p.to),
                ("block.s_root", b.s_root),
                ("block.length", b.length.unwrap_or(0.0)),
                ("block.x0", b.x0),
                ("block.y0", b.y0),
                ("block.height", b.height),
                ("block.width", b.width),
                ("block.inset", b.inset),
                ("block.chord_y", b.chord_y.unwrap_or(0.0)),
            ] {
                finite(&format!("panel {:?} {name}", p.name), v)?;
            }
        }
        self.stations.sort_by(|a, b| a.span.total_cmp(&b.span));
        for w in self.stations.windows(2) {
            ensure!(w[1].span > w[0].span, "two stations at span {}", w[0].span);
        }
        for s in &self.stations {
            ensure!(
                s.chord > 0.0,
                "station at span {}: chord must be > 0",
                s.span
            );
        }
        let (lo, hi) = (
            self.stations[0].span,
            self.stations[self.stations.len() - 1].span,
        );
        let mut names = std::collections::HashSet::new();
        for p in &self.panels {
            ensure!(names.insert(&p.name), "duplicate panel name {:?}", p.name);
            ensure!(
                !p.name.is_empty()
                    && p.name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)),
                "panel name {:?} must be non-empty and use only letters, digits, - _ .",
                p.name
            );
            if !(p.from >= lo && p.to <= hi && p.to > p.from) {
                bail!(
                    "panel {:?}: from/to ({}..{}) must be increasing and inside the stations ({lo}..{hi})",
                    p.name,
                    p.from,
                    p.to
                );
            }
            let b = &p.block;
            ensure!(
                b.height > 0.0 && b.width > 0.0,
                "panel {:?}: block height and width must be > 0",
                p.name
            );
            for (i, c) in p.cutouts.iter().enumerate() {
                let what = format!("panel {:?} cutout {}", p.name, i + 1);
                for (name, v) in [
                    ("x", c.x),
                    ("width", c.width),
                    ("depth", c.depth.unwrap_or(1.0)),
                    ("leave", c.leave.unwrap_or(1.0)),
                ] {
                    finite(&format!("{what} {name}"), v)?;
                }
                ensure!(
                    c.x > 0.0 && c.x < 1.0,
                    "{what}: x must be a chord fraction in (0, 1)"
                );
                ensure!(c.width > 0.0, "{what}: width must be > 0");
                match (c.depth, c.leave) {
                    (Some(d), None) => ensure!(d > 0.0, "{what}: depth must be > 0"),
                    (None, Some(l)) => ensure!(l >= 0.0, "{what}: leave must be >= 0"),
                    _ => {
                        bail!("{what}: set exactly one of depth (spar notch) or leave (hinge cut)")
                    }
                }
            }
            if let Some(l) = b.length {
                ensure!(
                    (l - (p.to - p.from)).abs() < 0.01,
                    "panel {:?}: block length {l} must equal the panel span (to - from = {}); the cut faces are the block faces",
                    p.name,
                    p.to - p.from
                );
            }
        }
        Ok(())
    }

    pub fn to_toml(&self) -> Result<String> {
        Ok(toml::to_string_pretty(self)?)
    }

    pub fn panel(&self, name: &str) -> Result<&Panel> {
        self.panels
            .iter()
            .find(|p| p.name == name)
            .with_context(|| {
                let names: Vec<&str> = self.panels.iter().map(|p| p.name.as_str()).collect();
                format!("no panel named {name:?} (have: {})", names.join(", "))
            })
    }
}
