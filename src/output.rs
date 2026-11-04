//! Writing a panel's G-code and SVG preview (shared by the CLI and GUI).

use crate::config::{Machine, Wing};
use crate::cut::CutPlan;
use crate::{gcode, svg};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Write `<dir>/<panel>.gcode` and `<dir>/<panel>.svg`; returns both paths.
pub fn write_panel(
    dir: &Path,
    plan: &CutPlan,
    machine: &Machine,
    wing: &Wing,
) -> Result<[PathBuf; 2]> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let panel = wing.panel(&plan.panel)?;
    let g = dir.join(format!("{}.gcode", plan.panel));
    let s = dir.join(format!("{}.svg", plan.panel));
    write_atomic(&g, &gcode::emit(plan, machine, &wing.name))?;
    write_atomic(&s, &svg::emit(plan, machine, panel))?;
    Ok([g, s])
}

/// Write via a temporary file and rename, so an interrupted run never leaves
/// a truncated G-code file that could be sent to the machine.
pub fn write_atomic(path: &Path, text: &str) -> Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))
}
