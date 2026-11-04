use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use std::io::{self, Write};
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
    /// Print a shell completion script, e.g. `wingcut completions bash > ~/.local/share/bash-completion/completions/wingcut`
    Completions {
        /// bash, zsh, fish, elvish or powershell
        shell: clap_complete::Shell,
    },
}

fn main() {
    if let Err(e) = run() {
        // `wingcut check | head` closes stdout early; that is not an error.
        let broken_pipe = e
            .downcast_ref::<io::Error>()
            .is_some_and(|io| io.kind() == io::ErrorKind::BrokenPipe);
        if !broken_pipe {
            eprintln!("error: {e:#}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<()> {
    let mut out = io::stdout().lock();
    match Cli::parse().cmd {
        Cmd::Gen { job, out: dir } => {
            let (m, model, plans) = plan(&job)?;
            std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
            for p in &plans {
                let panel = model.wing.panel(&p.panel)?;
                let g = dir.join(format!("{}.gcode", p.panel));
                let s = dir.join(format!("{}.svg", p.panel));
                write_atomic(&g, &gcode::emit(p, &m, &model.wing.name))?;
                write_atomic(&s, &svg::emit(p, &m, panel))?;
                summary(&mut out, p)?;
                writeln!(out, "  wrote {} and {}", g.display(), s.display())?;
            }
        }
        Cmd::Check { job } => {
            let (_, _, plans) = plan(&job)?;
            for p in &plans {
                summary(&mut out, p)?;
            }
            writeln!(out, "ok: {} panel(s) within limits", plans.len())?;
        }
        Cmd::Completions { shell } => {
            clap_complete::generate(shell, &mut Cli::command(), "wingcut", &mut out);
        }
        Cmd::Info { file } => {
            let af = Airfoil::load(&file)?;
            let st = af.stats();
            let fmt = match af.format {
                DatFormat::Selig => "Selig",
                DatFormat::Lednicer => "Lednicer",
            };
            let te = if af.is_open_te() { "open" } else { "closed" };
            writeln!(out, "{}", file.display())?;
            writeln!(out, "  name:          {}", af.name)?;
            writeln!(out, "  format:        {fmt}, {} points", af.n_points)?;
            writeln!(
                out,
                "  max thickness: {:.2} % at {:.1} % chord",
                st.max_thickness * 100.0,
                st.max_thickness_x * 100.0
            )?;
            writeln!(
                out,
                "  max camber:    {:.2} % at {:.1} % chord",
                st.max_camber * 100.0,
                st.max_camber_x * 100.0
            )?;
            writeln!(
                out,
                "  trailing edge: {te}, gap {:.3} % chord",
                af.te_gap * 100.0
            )?;
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
    // Plan every panel before writing anything: one bad panel writes nothing.
    let plans = panels
        .into_iter()
        .map(|p| plan_panel(&model, &m, p))
        .collect::<Result<Vec<_>>>()?;
    Ok((m, model, plans))
}

/// Write via a temporary file and rename, so an interrupted run never leaves
/// a truncated G-code file that could be sent to the machine.
fn write_atomic(path: &Path, text: &str) -> Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))
}

/// Per-panel summary on stdout; warnings go to stderr.
fn summary(out: &mut impl Write, p: &CutPlan) -> Result<()> {
    writeln!(out, "panel {}:", p.panel)?;
    for f in &p.faces {
        writeln!(
            out,
            "  face s={:<7.1} {:<9} path {:7.1} mm  speed {:5.1}..{:5.1} mm/min (avg {:5.1})  kerf {:.2} mm",
            f.s,
            f.role(),
            f.path_len,
            f.speed_min,
            f.speed_max,
            f.speed_avg,
            f.kerf
        )?;
    }
    writeln!(
        out,
        "  time {} (cutting {}), max tower speed {:.0} mm/min, {} moves",
        fmt_minutes(p.total_minutes),
        fmt_minutes(p.cut_minutes),
        p.max_tower_speed,
        p.moves.len()
    )?;
    for w in &p.warnings {
        eprintln!("warning: panel {}: {w}", p.panel);
    }
    Ok(())
}
