//! App state, file actions and the top/bottom bars.

use crate::{editors, preview};
use anyhow::{Context, Result};
use eframe::egui::{self, Color32, RichText};
use std::path::{Path, PathBuf};
use wingcut::config::{Block, Machine, Panel, Side, Station, Wing};
use wingcut::cut::{CutPlan, Violation, plan_panel, plan_panel_unchecked};
use wingcut::output;
use wingcut::wing::WingModel;

#[derive(PartialEq, Clone, Copy)]
pub enum SideTab {
    Wing,
    Machine,
    Control,
}

/// An action waiting for "save changes?" to be answered.
#[derive(PartialEq, Clone, Copy, Debug)]
pub enum Pending {
    Quit,
    NewWing,
    OpenWing,
}

#[derive(PartialEq, Clone, Copy)]
pub enum ViewTab {
    Cut,
    Planform,
    Front,
}

/// What the cut preview shows.
pub struct Layers {
    pub nominal: bool,
    pub kerfed: bool,
    pub towers: bool,
    pub block: bool,
    pub limits: bool,
}

/// A successfully planned panel, possibly outside the axis limits.
pub struct PreviewData {
    pub plan: CutPlan,
    pub violations: Vec<Violation>,
    pub panel: Panel,
    pub machine: Machine,
    pub millis: u128,
}

pub struct App {
    pub machine: Machine,
    pub machine_path: Option<PathBuf>,
    pub wing: Wing,
    pub wing_path: Option<PathBuf>,
    pub selected: usize,
    pub side_tab: SideTab,
    pub view_tab: ViewTab,
    pub layers: Layers,
    pub preview: Option<Result<PreviewData, String>>,
    /// Inputs changed since the last plan.
    pub dirty: bool,
    pub wing_unsaved: bool,
    pub machine_unsaved: bool,
    pub out_dir: Option<PathBuf>,
    pub status: String,
    pub pending: Option<Pending>,
    /// Machine control (connection, jog, running cuts).
    pub control: crate::control::Control,
    /// Asking whether to stop a running cut before quitting.
    confirm_stop_quit: bool,
    /// "Split into panels" dialog, when open.
    pub split: Option<crate::editors::SplitDialog>,
    allow_close: bool,
    title: String,
}

impl App {
    pub fn new(machine: Option<PathBuf>, wing: Option<PathBuf>) -> App {
        let mut app = App {
            machine: Machine::parse("").expect("defaults are valid"),
            machine_path: None,
            wing: new_wing(),
            wing_path: None,
            selected: 0,
            side_tab: SideTab::Wing,
            view_tab: ViewTab::Cut,
            layers: Layers {
                nominal: true,
                kerfed: true,
                towers: true,
                block: true,
                limits: false,
            },
            preview: None,
            dirty: true,
            wing_unsaved: false,
            machine_unsaved: false,
            out_dir: None,
            status: "Open a wing (File ▸ Open wing…) or edit this new one.".into(),
            pending: None,
            control: Default::default(),
            confirm_stop_quit: false,
            split: None,
            allow_close: false,
            title: String::new(),
        };
        // An explicit machine first, so the wing doesn't load its sibling.
        if let Some(m) = machine {
            app.open_machine(&m);
        }
        if let Some(w) = wing {
            app.open_wing(&w);
        }
        app
    }

    // ---- file actions ---------------------------------------------------

    pub fn open_wing(&mut self, path: &Path) {
        match Wing::read(path) {
            Ok(w) => {
                self.wing = w;
                self.wing_path = Some(path.to_path_buf());
                self.selected = 0;
                self.wing_unsaved = false;
                self.dirty = true;
                self.out_dir = None;
                self.status = format!("Opened {}", path.display());
                // Pick up the machine.toml next to the wing, unless a
                // machine file is already open.
                let sibling = path.parent().map(|d| d.join("machine.toml"));
                if self.machine_path.is_none()
                    && let Some(m) = sibling.filter(|p| p.exists())
                {
                    self.open_machine(&m);
                }
            }
            Err(e) => self.status = format!("⚠ {e:#}"),
        }
    }

    pub fn open_machine(&mut self, path: &Path) {
        match Machine::read(path) {
            Ok(m) => {
                self.machine = m;
                self.machine_path = Some(path.to_path_buf());
                self.machine_unsaved = false;
                self.dirty = true;
                self.status = format!("Opened {}", path.display());
            }
            Err(e) => self.status = format!("⚠ {e:#}"),
        }
    }

    pub fn save_wing(&mut self, ask: bool) {
        let path = match (&self.wing_path, ask) {
            (Some(p), false) => Some(p.clone()),
            _ => toml_dialog(self.wing_path.as_deref(), "wing.toml").save_file(),
        };
        let Some(path) = path else { return };
        // Keep airfoil paths pointing at the same files from the new place;
        // only adopt the change once the file is written.
        let mut w = self.wing.clone();
        w.base_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        for st in &mut w.stations {
            let abs = self.wing.base_dir.join(&st.airfoil);
            st.airfoil = relative_to(&abs, &w.base_dir);
        }
        match w.to_toml().and_then(|t| output::write_atomic(&path, &t)) {
            Ok(()) => {
                self.wing = w;
                self.wing_path = Some(path.clone());
                self.wing_unsaved = false;
                self.status = format!("Saved {}", path.display());
            }
            Err(e) => self.status = format!("⚠ {e:#}"),
        }
    }

    pub fn save_machine(&mut self, ask: bool) {
        let path = match (&self.machine_path, ask) {
            (Some(p), false) => Some(p.clone()),
            _ => toml_dialog(
                self.machine_path.as_deref().or(self.wing_path.as_deref()),
                "machine.toml",
            )
            .save_file(),
        };
        let Some(path) = path else { return };
        let res = self
            .machine
            .to_toml()
            .and_then(|t| output::write_atomic(&path, &t));
        match res {
            Ok(()) => {
                self.machine_path = Some(path.clone());
                self.machine_unsaved = false;
                self.status = format!("Saved {}", path.display());
            }
            Err(e) => self.status = format!("⚠ {e:#}"),
        }
    }

    /// Plan and write G-code + SVG for one panel or all of them.
    pub fn generate(&mut self, only_selected: bool) {
        let dir = self.output_dir();
        let res = (|| -> Result<Vec<PathBuf>> {
            let m = self
                .machine
                .clone()
                .normalized()
                .context("machine settings")?;
            let mut w = self.wing.clone();
            w.validate().context("wing")?;
            let model = WingModel::new(w, m.max_segment)?;
            let names: Vec<String> = if only_selected {
                vec![
                    self.selected_panel()
                        .context("no panel selected")?
                        .name
                        .clone(),
                ]
            } else {
                model.wing.panels.iter().map(|p| p.name.clone()).collect()
            };
            // Plan all first: one bad panel writes nothing.
            let plans = names
                .iter()
                .map(|n| plan_panel(&model, &m, model.wing.panel(n)?))
                .collect::<Result<Vec<_>>>()?;
            let mut files = Vec::new();
            for p in &plans {
                files.extend(output::write_panel(&dir, p, &m, &model.wing)?);
            }
            Ok(files)
        })();
        match res {
            Ok(files) => self.status = format!("Wrote {} files to {}", files.len(), dir.display()),
            Err(e) => self.status = format!("⚠ Not written: {e:#}"),
        }
    }

    /// Where Generate writes: the chosen folder, or `out/` next to the wing.
    pub fn output_dir(&self) -> PathBuf {
        self.out_dir
            .clone()
            .unwrap_or_else(|| self.wing.base_dir.join("out"))
    }

    pub fn selected_panel(&self) -> Option<&Panel> {
        self.wing.panels.get(self.selected)
    }

    pub fn replan(&mut self) {
        let t0 = std::time::Instant::now();
        let res = (|| -> Result<PreviewData> {
            let m = self
                .machine
                .clone()
                .normalized()
                .context("machine settings")?;
            let mut w = self.wing.clone();
            w.validate().context("wing")?;
            let name = self
                .selected_panel()
                .context("no panel: add one under Panels")?
                .name
                .clone();
            let model = WingModel::new(w, m.max_segment)?;
            let panel = model.wing.panel(&name)?.clone();
            let (plan, limits) = plan_panel_unchecked(&model, &m, &panel)?;
            Ok(PreviewData {
                plan,
                violations: limits.map(|l| l.violations).unwrap_or_default(),
                panel,
                machine: m,
                millis: 0,
            })
        })();
        self.preview = Some(
            res.map(|mut p| {
                p.millis = t0.elapsed().as_millis();
                if !p.violations.is_empty() {
                    self.layers.limits = true;
                }
                p
            })
            .map_err(|e| format!("{e:#}")),
        );
    }

    // ---- unsaved-changes guard -----------------------------------------

    /// Does `action` throw away unsaved edits?
    fn loses_edits(&self, action: Pending) -> bool {
        match action {
            Pending::Quit => self.wing_unsaved || self.machine_unsaved,
            Pending::NewWing | Pending::OpenWing => self.wing_unsaved,
        }
    }

    /// Run `action` now, or ask first if it would lose edits.
    pub fn request(&mut self, action: Pending, ctx: &egui::Context) {
        if action == Pending::Quit && self.control.running() {
            self.confirm_stop_quit = true;
            return;
        }
        if self.loses_edits(action) {
            self.pending = Some(action);
        } else {
            self.run(action, ctx);
        }
    }

    fn run(&mut self, action: Pending, ctx: &egui::Context) {
        self.pending = None;
        match action {
            Pending::Quit => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Pending::NewWing => {
                self.wing = new_wing();
                self.wing_path = None;
                self.selected = 0;
                self.wing_unsaved = true;
                self.dirty = true;
            }
            Pending::OpenWing => {
                if let Some(p) = toml_dialog(self.wing_path.as_deref(), "").pick_file() {
                    self.open_wing(&p);
                }
            }
        }
    }

    fn unsaved_dialog(&mut self, ctx: &egui::Context) {
        let Some(action) = self.pending else { return };
        let mut files = Vec::new();
        if self.wing_unsaved {
            files.push("the wing");
        }
        if self.machine_unsaved && action == Pending::Quit {
            files.push("the machine settings");
        }
        let choice = egui::Modal::new(egui::Id::unique("unsaved"))
            .show(ctx, |ui| {
                ui.heading("Save changes?");
                ui.label(format!(
                    "You have unsaved changes to {}.",
                    files.join(" and ")
                ));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        Some(true)
                    } else if ui.button("Discard").clicked() {
                        Some(false)
                    } else if ui.button("Cancel").clicked() {
                        self.pending = None;
                        None
                    } else {
                        None
                    }
                })
                .inner
            })
            .inner;
        match choice {
            Some(true) => {
                if self.wing_unsaved {
                    self.save_wing(false);
                }
                if action == Pending::Quit && self.machine_unsaved {
                    self.save_machine(false);
                }
                // Saving was cancelled or failed: keep the edits, stay put.
                if self.loses_edits(action) {
                    self.pending = None;
                } else {
                    self.run(action, ctx);
                }
            }
            Some(false) => self.run(action, ctx),
            None => {}
        }
    }

    fn stop_quit_dialog(&mut self, ctx: &egui::Context) {
        if !self.confirm_stop_quit {
            return;
        }
        if !self.control.running() {
            self.confirm_stop_quit = false;
            return;
        }
        egui::Modal::new(egui::Id::unique("stop-quit")).show(ctx, |ui| {
            ui.heading("A cut is running");
            ui.label("Quitting stops the cut (hold, then reset: the wire switches off).");
            ui.horizontal(|ui| {
                if ui.button("Stop the cut and quit").clicked() {
                    self.control.stop();
                    self.control.quit_after_stop = true;
                    self.confirm_stop_quit = false;
                }
                if ui.button("Keep cutting").clicked() {
                    self.confirm_stop_quit = false;
                }
            });
        });
    }

    // ---- frame ----------------------------------------------------------

    fn shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, KeyboardShortcut, Modifiers};
        let save = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
        if ctx.input_mut(|i| i.consume_shortcut(&save)) {
            if self.wing_unsaved || self.wing_path.is_none() {
                self.save_wing(false);
            }
            if self.machine_unsaved {
                self.save_machine(false);
            }
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let name = |p: &Option<PathBuf>, unsaved: bool, fallback: &str| {
            let n = p
                .as_ref()
                .and_then(|p| p.file_name())
                .map_or(fallback.to_string(), |f| f.to_string_lossy().into_owned());
            if unsaved { format!("{n}*") } else { n }
        };
        let title = format!(
            "wingcut – {} + {}",
            name(&self.wing_path, self.wing_unsaved, "new wing"),
            name(&self.machine_path, self.machine_unsaved, "default machine")
        );
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New wing").clicked() {
                    self.request(Pending::NewWing, ui.ctx());
                    ui.close();
                }
                if ui.button("Open wing…").clicked() {
                    self.request(Pending::OpenWing, ui.ctx());
                    ui.close();
                }
                if ui.button("Save wing          Ctrl+S").clicked() {
                    self.save_wing(false);
                    ui.close();
                }
                if ui.button("Save wing as…").clicked() {
                    self.save_wing(true);
                    ui.close();
                }
                ui.separator();
                if ui.button("Open machine…").clicked() {
                    if let Some(p) = toml_dialog(self.machine_path.as_deref(), "").pick_file() {
                        self.open_machine(&p);
                    }
                    ui.close();
                }
                if ui.button("Save machine").clicked() {
                    self.save_machine(false);
                    ui.close();
                }
                if ui.button("Save machine as…").clicked() {
                    self.save_machine(true);
                    ui.close();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    self.request(Pending::Quit, ui.ctx());
                    ui.close();
                }
            });
            crate::help::menu(ui);
            ui.separator();
            if ui
                .button("⚙ Generate panel")
                .on_hover_text("Write G-code + SVG for the selected panel")
                .clicked()
            {
                self.generate(true);
            }
            if ui
                .button("⚙ Generate all")
                .on_hover_text("Write G-code + SVG for every panel")
                .clicked()
            {
                self.generate(false);
            }
            let out = format!("→ {}", self.output_dir().display());
            ui.add(egui::Label::new(RichText::new(&out).weak()).truncate())
                .on_hover_text(format!("Generate writes the G-code and SVG files to {out}"));
            if ui
                .small_button("change…")
                .on_hover_text("Choose the output folder")
                .clicked()
                && let Some(d) = rfd::FileDialog::new()
                    .set_directory(self.output_dir())
                    .pick_folder()
            {
                self.out_dir = Some(d);
            }
            ui.separator();
            let color = if self.status.starts_with('⚠') {
                Color32::from_rgb(220, 80, 60)
            } else {
                ui.visuals().weak_text_color()
            };
            // Truncated so a long message never widens the window.
            ui.add(egui::Label::new(RichText::new(&self.status).color(color)).truncate())
                .on_hover_text(&self.status);
        });
    }

    fn bottom_bar(&self, ui: &mut egui::Ui) {
        match &self.preview {
            None => {
                ui.label("…");
            }
            Some(Err(e)) => {
                ui.label(RichText::new(format!("⚠ {e}")).color(Color32::from_rgb(220, 80, 60)));
            }
            Some(Ok(p)) => {
                let plan = &p.plan;
                ui.horizontal_wrapped(|ui| {
                    for f in &plan.faces {
                        ui.label(format!(
                            "face s={:.0} ({}): path {:.0} mm, speed {:.0}–{:.0} mm/min, kerf {:.2} mm",
                            f.s,
                            f.role(),
                            f.path_len,
                            f.speed_min,
                            f.speed_max,
                            f.kerf
                        ));
                        ui.separator();
                    }
                    ui.label(format!(
                        "time {}, max tower speed {:.0} mm/min, {} moves, planned in {} ms",
                        wingcut::gcode::fmt_minutes(plan.total_minutes),
                        plan.max_tower_speed,
                        plan.moves.len(),
                        p.millis
                    ));
                });
                for w in &plan.warnings {
                    ui.label(RichText::new(format!("⚠ {w}")).color(Color32::from_rgb(200, 150, 0)));
                }
                if !p.violations.is_empty() {
                    let v = &p.violations[0];
                    ui.label(
                        RichText::new(format!(
                            "⚠ {} point(s) outside the axis limits (red in the preview) – e.g. {} tower {} = {:.1} outside [{}, {}] at {}. Generate is blocked.",
                            p.violations.len(),
                            v.tower,
                            v.axis,
                            v.value,
                            v.limits[0],
                            v.limits[1],
                            v.location
                        ))
                        .color(Color32::from_rgb(220, 60, 60)),
                    );
                }
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let machine = self.machine.clone();
        let job_ended = self.control.poll(&ctx, &machine);
        if job_ended && self.control.quit_after_stop {
            self.control.quit_after_stop = false;
            self.request(Pending::Quit, &ctx);
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            if self.control.running() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.confirm_stop_quit = true;
            } else if self.loses_edits(Pending::Quit) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.pending = Some(Pending::Quit);
            }
        }
        self.shortcuts(&ctx);
        egui::Panel::top("top").show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("bottom").show(ui, |ui| {
            ui.add_space(4.0);
            self.bottom_bar(ui);
            ui.add_space(4.0);
        });
        egui::Panel::left("editors")
            .resizable(true)
            .default_size(560.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.side_tab, SideTab::Wing, "Wing");
                    ui.selectable_value(&mut self.side_tab, SideTab::Machine, "Machine");
                    let label = if self.control.connected() {
                        "Control ●"
                    } else {
                        "Control"
                    };
                    ui.selectable_value(&mut self.side_tab, SideTab::Control, label)
                        .on_hover_text("Connect to the machine, jog, run a cut");
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        let changed = match self.side_tab {
                            SideTab::Wing => editors::wing(ui, self),
                            SideTab::Machine => editors::machine(ui, &mut self.machine),
                            SideTab::Control => crate::control::ui(ui, self),
                        };
                        if changed {
                            self.dirty = true;
                            match self.side_tab {
                                SideTab::Wing => self.wing_unsaved = true,
                                SideTab::Machine | SideTab::Control => self.machine_unsaved = true,
                            }
                        }
                    });
            });
        if self.dirty {
            self.dirty = false;
            self.replan();
        }
        egui::CentralPanel::default().show(ui, |ui| preview::show(ui, self));
        self.unsaved_dialog(&ctx);
        crate::control::preflight_dialog(&ctx, &mut self.control);
        crate::editors::split_dialog(&ctx, self);
        self.stop_quit_dialog(&ctx);
        self.update_title(&ctx);
    }
}

/// A fresh wing: two stations and one panel (airfoils still to choose).
pub fn new_wing() -> Wing {
    let station = |span: f64, chord: f64| Station {
        span,
        chord,
        airfoil: PathBuf::new(),
        le_offset: 0.0,
        dihedral_offset: 0.0,
        twist_deg: 0.0,
        twist_pivot: 0.25,
    };
    Wing {
        name: "new-wing".into(),
        units: "mm".into(),
        stations: vec![station(0.0, 250.0), station(1000.0, 200.0)],
        panels: vec![new_panel("panel-1", 0.0, 1000.0)],
        base_dir: std::env::current_dir().unwrap_or_default(),
    }
}

pub fn new_panel(name: &str, from: f64, to: f64) -> Panel {
    Panel {
        name: name.into(),
        from,
        to,
        side: Side::Left,
        block: Block {
            s_root: 300.0,
            length: None,
            x0: 100.0,
            y0: 20.0,
            height: 60.0,
            width: 300.0,
            inset: 10.0,
            chord_y: None,
        },
        wire_current: None,
        sheeting: Default::default(),
        cutouts: Vec::new(),
    }
}

pub fn toml_dialog(near: Option<&Path>, file_name: &str) -> rfd::FileDialog {
    let mut d = rfd::FileDialog::new().add_filter("TOML", &["toml"]);
    if let Some(dir) = near.and_then(Path::parent) {
        d = d.set_directory(dir);
    }
    if !file_name.is_empty() {
        d = d.set_file_name(file_name);
    }
    d
}

/// `path` relative to `base` when it lies inside it, else absolute.
pub fn relative_to(path: &Path, base: &Path) -> PathBuf {
    path.strip_prefix(base)
        .map_or_else(|_| path.to_path_buf(), Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example() -> App {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples");
        App::new(None, Some(dir.join("wing.toml")))
    }

    #[test]
    fn opens_the_example_with_its_machine_and_plans_it() {
        let mut app = example();
        assert!(
            app.machine_path
                .as_ref()
                .is_some_and(|p| p.ends_with("machine.toml"))
        );
        assert_eq!(app.wing.panels.len(), 4);
        app.replan();
        let p = app.preview.as_ref().unwrap().as_ref().unwrap();
        assert_eq!(p.plan.panel, "center-L");
        assert!(p.violations.is_empty());
    }

    #[test]
    fn limit_violations_are_previewed_not_hidden() {
        let mut app = example();
        app.wing.panels[0].block.y0 = -15.0;
        app.replan();
        let p = app.preview.as_ref().unwrap().as_ref().expect("still plans");
        assert!(!p.violations.is_empty());
        assert!(
            app.layers.limits,
            "limits are switched on to show the problem"
        );
    }

    #[test]
    fn errors_are_reported_not_panicked() {
        let mut app = example();
        app.wing.stations[0].chord = -1.0;
        app.replan();
        let e = app
            .preview
            .as_ref()
            .unwrap()
            .as_ref()
            .err()
            .expect("an error");
        assert!(e.contains("chord must be > 0"), "{e}");
        // A fresh wing has no airfoils yet: also an error, not a crash.
        let mut app = App::new(None, None);
        app.replan();
        assert!(app.preview.as_ref().unwrap().is_err());
    }

    #[test]
    fn generate_writes_all_panels_or_nothing() {
        let mut app = example();
        let dir = std::env::temp_dir().join(format!("wingcut-gui-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        app.out_dir = Some(dir.clone());
        app.wing.panels[1].block.y0 = -15.0; // tip-L breaks the limits
        app.generate(false);
        assert!(app.status.contains("Not written"), "{}", app.status);
        assert!(!dir.exists());
        app.wing.panels[1].block.y0 = 20.0;
        app.generate(false);
        assert!(app.status.contains("Wrote 8 files"), "{}", app.status);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unsaved_edits_are_not_thrown_away_without_asking() {
        let ctx = egui::Context::default();
        let mut app = example();
        app.request(Pending::NewWing, &ctx);
        assert_eq!(app.pending, None, "nothing to lose: runs at once");
        assert_eq!(app.wing.name, "new-wing");
        let mut app = example();
        app.wing.name = "edited".into();
        app.wing_unsaved = true;
        app.request(Pending::NewWing, &ctx);
        assert_eq!(app.pending, Some(Pending::NewWing));
        assert_eq!(app.wing.name, "edited", "still there until answered");
        // Machine edits only matter when quitting.
        let mut app = example();
        app.machine_unsaved = true;
        app.request(Pending::Quit, &ctx);
        assert_eq!(app.pending, Some(Pending::Quit));
    }

    /// Poll the control until `done` or time out.
    fn pump(app: &mut App, ctx: &egui::Context, secs: f64, mut done: impl FnMut(&App) -> bool) {
        let t0 = std::time::Instant::now();
        while !done(app) {
            assert!(
                t0.elapsed().as_secs_f64() < secs,
                "timed out: {}",
                app.control.message
            );
            let m = app.machine.clone();
            app.control.poll(ctx, &m);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn runs_a_panel_on_the_simulator_with_preflight_checks() {
        use crate::control::{Check, selected_job};
        use wingcut::config::LinkKind;
        let ctx = egui::Context::default();
        let mut app = example();
        app.machine.connection.kind = LinkKind::Simulator;
        app.machine.connection.sim_speed = 2000.0;
        let m = app.machine.clone();
        app.control.connect(&m);
        pump(&mut app, &ctx, 5.0, |a| a.control.connected());
        // Starts in alarm (homing required): the pre-flight check blocks.
        pump(&mut app, &ctx, 5.0, |a| a.control.status.is_some());
        let job = selected_job(&app);
        app.control.prepare(&m, job);
        let pf = app.control.preflight.take().unwrap();
        assert!(
            pf.checks
                .iter()
                .any(|(c, t)| *c == Check::Block && t.contains("Alarm"))
        );
        // Home, then everything passes (the sim homes in front of the block).
        app.control.home();
        pump(&mut app, &ctx, 5.0, |a| a.control.homed);
        pump(&mut app, &ctx, 5.0, |a| {
            a.control
                .status
                .as_ref()
                .is_some_and(|s| s.state == wingcut_machine::grbl::State::Idle)
        });
        let job = selected_job(&app);
        app.control.prepare(&m, job);
        let pf = app.control.preflight.as_ref().unwrap();
        assert!(
            !pf.checks.iter().any(|(c, _)| *c == Check::Block),
            "{:?}",
            pf.checks.iter().map(|c| &c.1).collect::<Vec<_>>()
        );
        app.control.start_prepared();
        pump(&mut app, &ctx, 10.0, |a| a.control.running());
        pump(&mut app, &ctx, 30.0, |a| !a.control.running());
        assert!(
            app.control.message.contains("finished"),
            "{}",
            app.control.message
        );
    }

    #[test]
    fn riden_sets_the_current_and_a_broken_wire_pauses_the_cut() {
        use crate::control::{Check, selected_job};
        use wingcut::config::LinkKind;
        let ctx = egui::Context::default();
        let mut app = example();
        app.machine.connection.kind = LinkKind::Simulator;
        app.machine.connection.sim_speed = 20.0;
        app.machine.riden.enabled = true;
        app.machine.riden.simulated = true;
        app.machine.riden.current = 1.9;
        app.wing.panels[0].wire_current = Some(2.1);
        let m = app.machine.clone();
        // The Riden must be connected, or the cut is blocked.
        app.control.connect(&m);
        pump(&mut app, &ctx, 5.0, |a| a.control.connected());
        app.control.home();
        pump(&mut app, &ctx, 5.0, |a| a.control.homed);
        pump(&mut app, &ctx, 5.0, |a| {
            a.control
                .status
                .as_ref()
                .is_some_and(|s| s.state == wingcut_machine::grbl::State::Idle)
        });
        app.control.prepare(&m, selected_job(&app));
        assert!(
            app.control
                .preflight
                .as_ref()
                .unwrap()
                .checks
                .iter()
                .any(|(c, t)| *c == Check::Block && t.contains("Riden"))
        );
        app.control.connect_riden(&ctx, &m);
        pump(&mut app, &ctx, 5.0, |a| a.control.reading.is_some());
        app.control.prepare(&m, selected_job(&app));
        let pf = app.control.preflight.as_ref().unwrap();
        assert!(
            pf.checks
                .iter()
                .any(|(_, t)| t.contains("2.10 A (panel setting)")),
            "panel current wins"
        );
        assert!(!pf.checks.iter().any(|(c, _)| *c == Check::Block));
        app.control.start_prepared();
        let fake = app.control.fake_riden.clone().unwrap();
        // Current flows once the board switches the wire on.
        pump(&mut app, &ctx, 10.0, |a| {
            a.control.reading.is_some_and(|r| r.output && r.i_out > 2.0)
        });
        assert!((fake.lock().unwrap().current_set() - 2.1).abs() < 1e-9);
        // Break the wire: the cut pauses and everything goes off.
        fake.lock().unwrap().wire_ohms = None;
        pump(&mut app, &ctx, 10.0, |a| {
            a.control.job.as_ref().is_some_and(|j| j.paused)
        });
        assert!(
            app.control.message.contains("broken wire"),
            "{}",
            app.control.message
        );
        pump(&mut app, &ctx, 5.0, |_| !fake.lock().unwrap().output_on());
        let sim = app.control.sim.clone().unwrap();
        pump(&mut app, &ctx, 5.0, |_| !sim.lock().unwrap().wire_on());
        // Mend and resume, then stop: the supply ends switched off.
        fake.lock().unwrap().wire_ohms = Some(15.0);
        app.control.resume();
        pump(&mut app, &ctx, 5.0, |_| fake.lock().unwrap().output_on());
        pump(&mut app, &ctx, 5.0, |a| {
            a.control.job.as_ref().is_some_and(|j| !j.paused)
        });
        app.control.stop();
        pump(&mut app, &ctx, 10.0, |a| !a.control.running());
        pump(&mut app, &ctx, 5.0, |_| !fake.lock().unwrap().output_on());
    }

    #[test]
    fn preflight_blocks_when_the_wire_is_behind_the_block() {
        use crate::control::{Check, selected_job};
        use wingcut::config::LinkKind;
        let ctx = egui::Context::default();
        let mut app = example();
        app.machine.connection.kind = LinkKind::Simulator;
        app.machine.connection.sim_speed = 2000.0;
        let m = app.machine.clone();
        app.control.connect(&m);
        pump(&mut app, &ctx, 5.0, |a| a.control.connected());
        app.control.home();
        pump(&mut app, &ctx, 5.0, |a| a.control.homed);
        app.control.jog(&[('X', 300.0), ('Z', 300.0)], 2000.0);
        pump(&mut app, &ctx, 5.0, |a| {
            a.control.mpos.as_ref().is_some_and(|p| p[0] >= 300.0)
        });
        let job = selected_job(&app);
        app.control.prepare(&m, job);
        let pf = app.control.preflight.as_ref().unwrap();
        assert!(
            pf.checks
                .iter()
                .any(|(c, t)| *c == Check::Block && t.contains("not in front of the block"))
        );
    }

    #[test]
    fn airfoil_paths_are_kept_relative_when_possible() {
        let base = Path::new("/w");
        assert_eq!(
            relative_to(Path::new("/w/airfoils/a.dat"), base),
            PathBuf::from("airfoils/a.dat")
        );
        assert_eq!(
            relative_to(Path::new("/x/a.dat"), base),
            PathBuf::from("/x/a.dat")
        );
    }
}
