use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use wingcut::airfoil::{Airfoil, DatFormat};
use wingcut::config::{Machine, Wing};
use wingcut::cut::{CutPlan, plan_panel};
use wingcut::gcode::{self, fmt_minutes};
use wingcut::svg;
use wingcut::wing::WingModel;

#[derive(Parser)]
#[command(
    version,
    about = "Wing definition -> 4-axis G93 G-code for a CNC hot-wire foam cutter"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(clap::Args)]
struct Job {
    /// Machine config (machine.toml)
    #[arg(long)]
    machine: PathBuf,
    /// Wing definition (wing.toml)
    #[arg(long)]
    wing: PathBuf,
    /// Only this panel (default: all panels)
    #[arg(long)]
    panel: Option<String>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Generate G-code and SVG previews
    Gen {
        #[command(flatten)]
        job: Job,
        /// Output directory
        #[arg(short, long, default_value = "out")]
        out: PathBuf,
    },
    /// Validate configs and axis limits without writing anything
    Check {
        #[command(flatten)]
        job: Job,
    },
    /// Parse an airfoil .dat file and print its stats
    Info { file: PathBuf },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Gen { job, out } => {
            let (m, model, plans) = plan(&job)?;
            std::fs::create_dir_all(&out).with_context(|| format!("creating {}", out.display()))?;
            for p in &plans {
                let panel = model.wing.panel(&p.panel)?;
                let g = out.join(format!("{}.gcode", p.panel));
                let s = out.join(format!("{}.svg", p.panel));
                write(&g, &gcode::emit(p, &m, &model.wing.name))?;
                write(&s, &svg::emit(p, &m, panel))?;
                summary(p);
                println!("  wrote {} and {}", g.display(), s.display());
            }
        }
        Cmd::Check { job } => {
            let (_, _, plans) = plan(&job)?;
            for p in &plans {
                summary(p);
            }
            println!("ok: {} panel(s) within limits", plans.len());
        }
        Cmd::Info { file } => {
            let af = Airfoil::load(&file)?;
            let st = af.stats();
            println!("{}", file.display());
            println!("  name:          {}", af.name);
            let fmt = match af.format {
                DatFormat::Selig => "Selig",
                DatFormat::Lednicer => "Lednicer",
            };
            println!("  format:        {fmt}, {} points", af.n_points);
            println!(
                "  max thickness: {:.2} % at {:.1} % chord",
                st.max_thickness * 100.0,
                st.max_thickness_x * 100.0
            );
            println!(
                "  max camber:    {:.2} % at {:.1} % chord",
                st.max_camber * 100.0,
                st.max_camber_x * 100.0
            );
            let te = if af.is_open_te() { "open" } else { "closed" };
            println!(
                "  trailing edge: {te}, gap {:.3} % chord",
                af.te_gap * 100.0
            );
        }
    }
    Ok(())
}

fn plan(job: &Job) -> Result<(Machine, WingModel, Vec<CutPlan>)> {
    let m = Machine::load(&job.machine)?;
    let wing = Wing::load(&job.wing)?;
    let model = WingModel::new(wing, m.max_segment)?;
    let panels = match &job.panel {
        Some(name) => vec![model.wing.panel(name)?],
        None => model.wing.panels.iter().collect(),
    };
    anyhow::ensure!(!panels.is_empty(), "the wing defines no [[panel]]s");
    let plans = panels
        .into_iter()
        .map(|p| plan_panel(&model, &m, p))
        .collect::<Result<Vec<_>>>()?;
    Ok((m, model, plans))
}

fn write(path: &Path, text: &str) -> Result<()> {
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

fn summary(p: &CutPlan) {
    println!("panel {}:", p.panel);
    for f in &p.faces {
        println!(
            "  face s={:<7.1} {:<9} path {:7.1} mm  speed {:5.1}..{:5.1} mm/min (avg {:5.1})  kerf {:.2} mm",
            f.s,
            if f.is_wing_root {
                "wing root"
            } else {
                "wing tip"
            },
            f.path_len,
            f.speed_min,
            f.speed_max,
            f.speed_avg,
            f.kerf
        );
    }
    println!(
        "  time {} (cutting {}), max tower speed {:.0} mm/min, {} moves",
        fmt_minutes(p.total_minutes),
        fmt_minutes(p.cut_minutes),
        p.max_tower_speed,
        p.moves.len()
    );
    for w in &p.warnings {
        println!("  warning: {w}");
    }
}
