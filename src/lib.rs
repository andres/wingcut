//! wingcut: wing definition → synchronised 4-axis G93 G-code for a
//! two-tower CNC hot-wire foam cutter.
//!
//! The pipeline per panel is: parse configs ([`config`]), build the wing
//! model ([`wing::WingModel`]), plan the cut ([`cut::plan_panel`]), then
//! emit G-code ([`gcode::emit`]) and an SVG preview ([`svg::emit`]).
//!
//! ```
//! use std::path::Path;
//! use wingcut::{config::{Machine, Wing}, cut::plan_panel, gcode, wing::WingModel};
//!
//! let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
//! let machine = Machine::load(&examples.join("machine.toml"))?;
//! let wing = Wing::load(&examples.join("wing.toml"))?;
//! let model = WingModel::new(wing, machine.max_segment)?;
//!
//! let panel = model.wing.panel("center-L")?;
//! let plan = plan_panel(&model, &machine, panel)?;
//! let program = gcode::emit(&plan, &machine, &model.wing.name);
//!
//! assert!(program.contains("G93"));
//! assert!(plan.faces[1].kerf > plan.faces[0].kerf); // the tip face moves slower
//! # Ok::<(), anyhow::Error>(())
//! ```

pub mod airfoil;
pub mod config;
pub mod cut;
pub mod dxf;
pub mod gcode;
pub mod geom;
pub mod output;
pub mod path;
pub mod sheet;
pub mod svg;
pub mod wing;
