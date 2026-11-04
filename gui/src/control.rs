//! Machine control tab: connect, home, jog, run a panel with pre-flight
//! checks, pause/resume/stop, console.

use crate::app::App;
use eframe::egui::{self, Color32, ProgressBar, RichText, Ui};
use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use wingcut::config::{LinkKind, Machine};
use wingcut::cut::{CutPlan, plan_panel};
use wingcut::wing::WingModel;
use wingcut_machine::grbl::{State, Status};
use wingcut_machine::riden::{
    BreakDetector, FakeRiden, FakeRidenLink, Reading, RidenCommand, RidenEvent, RidenHandle,
};
use wingcut_machine::{
    Command, Controller, Event, Job, Link, Options, SerialLink, Sim, SimConfig, SimLink, TcpLink,
    WsLink,
};

type Opened = Result<(Box<dyn Link>, Option<Arc<Mutex<Sim>>>), String>;

const LOG_LINES: usize = 300;

#[derive(PartialEq, Clone, Copy)]
pub enum JogMode {
    Both,
    Root,
    Tip,
}

/// A running (or just finished) job as shown in the UI.
pub struct JobView {
    pub name: String,
    pub total: usize,
    pub acked: usize,
    pub total_secs: f64,
    pub remaining_secs: f64,
    pub paused: bool,
}

/// Pre-flight check result shown before a cut starts.
pub struct Preflight {
    pub job: Job,
    pub checks: Vec<(Check, String)>,
    /// Wire current and voltage limit to set on the Riden at start.
    pub riden: Option<(f64, f64)>,
}

#[derive(PartialEq, Clone, Copy)]
pub enum Check {
    Ok,
    Warn,
    Block,
}

pub struct Control {
    pub ctl: Option<Controller>,
    connecting: Option<Receiver<Opened>>,
    /// The simulated board, when connected to one.
    pub sim: Option<Arc<Mutex<Sim>>>,
    pub riden: Option<RidenHandle>,
    pub riden_model: String,
    pub reading: Option<Reading>,
    pub riden_msg: String,
    pub fake_riden: Option<Arc<Mutex<FakeRiden>>>,
    detector: BreakDetector,
    pub ports: Vec<String>,
    pub status: Option<Status>,
    pub mpos: Option<Vec<f64>>,
    pub homed: bool,
    pub log: VecDeque<String>,
    pub input: String,
    pub job: Option<JobView>,
    pub step: f64,
    pub feed: f64,
    pub mode: JogMode,
    pub dry_run: bool,
    pub preflight: Option<Preflight>,
    pub message: String,
    /// Close the app once the running job has stopped.
    pub quit_after_stop: bool,
}

impl Default for Control {
    fn default() -> Self {
        Control {
            ctl: None,
            connecting: None,
            sim: None,
            riden: None,
            riden_model: String::new(),
            reading: None,
            riden_msg: String::new(),
            fake_riden: None,
            detector: BreakDetector::new(0.5),
            ports: wingcut_machine::serial_ports(),
            status: None,
            mpos: None,
            homed: false,
            log: VecDeque::new(),
            input: String::new(),
            job: None,
            step: 10.0,
            feed: 600.0,
            mode: JogMode::Both,
            dry_run: false,
            preflight: None,
            message: String::new(),
            quit_after_stop: false,
        }
    }
}

/// Axis letters as chars.
pub fn axes(m: &Machine) -> [char; 4] {
    std::array::from_fn(|i| m.axes[i].chars().next().unwrap_or('X').to_ascii_uppercase())
}

/// Index of an axis letter in a FluidNC status report (axes are reported
/// in X Y Z A B C U V W order).
pub fn axis_index(c: char) -> usize {
    "XYZABCUVW".find(c).unwrap_or(0)
}

/// Root and tip tower positions (x, y) from a machine position.
pub fn towers(m: &Machine, mpos: &[f64]) -> Option<[[f64; 2]; 2]> {
    let a = axes(m);
    let get = |c: char| mpos.get(axis_index(c)).copied();
    Some([[get(a[0])?, get(a[1])?], [get(a[2])?, get(a[3])?]])
}

fn open_link(m: &Machine) -> Opened {
    let c = &m.connection;
    let mut sim = None;
    let r: anyhow::Result<Box<dyn Link>> = match c.kind {
        LinkKind::Serial => {
            if c.port.is_empty() {
                return Err("choose a serial port first".into());
            }
            SerialLink::open(&c.port, c.baud).map(|l| Box::new(l) as Box<dyn Link>)
        }
        LinkKind::Websocket => {
            WsLink::open(&c.host, c.websocket_port).map(|l| Box::new(l) as Box<dyn Link>)
        }
        LinkKind::Telnet => {
            TcpLink::open(&c.host, c.telnet_port).map(|l| Box::new(l) as Box<dyn Link>)
        }
        LinkKind::Simulator => {
            let a = axes(m);
            let (x, y) = (m.limits.x, m.limits.y);
            let cfg = SimConfig {
                axes: a,
                limits: [x, y, x, y],
                rapid: m.feed_rapid.max(1.0) * 1.25,
                speed: c.sim_speed,
                require_homing: true,
                // Home in front of the bed, wire at the top.
                home: [x[0], y[1], x[0], y[1]],
                start: [x[0], y[1], x[0], y[1]],
                ..SimConfig::default()
            };
            let (link, handle) = SimLink::new(cfg);
            sim = Some(handle);
            Ok(Box::new(link))
        }
    };
    r.map(|l| (l, sim)).map_err(|e| format!("{e:#}"))
}

impl Control {
    pub fn connected(&self) -> bool {
        self.ctl.is_some()
    }

    pub fn running(&self) -> bool {
        self.job.is_some()
    }

    fn send(&self, c: Command) {
        if let Some(ctl) = &self.ctl {
            ctl.send(c);
        }
    }

    fn log(&mut self, line: impl Into<String>) {
        self.log.push_back(line.into());
        while self.log.len() > LOG_LINES {
            self.log.pop_front();
        }
    }

    pub fn stop(&mut self) {
        self.send(Command::Stop);
    }

    pub fn home(&self) {
        self.send(Command::Home);
    }

    pub fn jog(&self, moves: &[(char, f64)], feed: f64) {
        self.send(Command::Jog {
            moves: moves.to_vec(),
            feed,
        });
    }

    /// Start the job prepared by [`Control::prepare`]. With a Riden, the
    /// voltage limit and current are set and its output switched on first
    /// (the board's M3/M5 still switch the wire through the MOSFET).
    pub fn start_prepared(&mut self) {
        if let Some(pf) = self.preflight.take() {
            if let (Some((amps, volts)), Some(r)) = (pf.riden, &self.riden) {
                r.send(RidenCommand::SetVoltage(volts));
                r.send(RidenCommand::SetCurrent(amps));
                r.send(RidenCommand::Output(true));
            }
            self.send(Command::Start(pf.job));
        }
    }

    /// Resume a paused cut (switching the Riden back on first).
    pub fn resume(&mut self) {
        if let Some(r) = &self.riden {
            r.send(RidenCommand::Output(true));
        }
        self.detector = BreakDetector::new(self.detector.fraction);
        self.send(Command::Resume);
    }

    fn riden_off(&self) {
        if let Some(r) = &self.riden {
            r.send(RidenCommand::Output(false));
        }
    }

    pub fn connect_riden(&mut self, ctx: &egui::Context, m: &Machine) {
        let rc = &m.riden;
        let link: Box<dyn Link> = if rc.simulated {
            // The simulated supply only carries current while the
            // simulated board has the wire on (its MOSFET).
            let sim = self.sim.clone();
            let load = Box::new(move || {
                sim.as_ref()
                    .is_none_or(|s| s.lock().map(|s| s.wire_on()).unwrap_or(false))
            });
            let fake = Arc::new(Mutex::new(FakeRiden::new(load)));
            fake.lock().expect("new lock").addr = rc.address;
            self.fake_riden = Some(fake.clone());
            Box::new(FakeRidenLink(fake))
        } else {
            if rc.port.is_empty() {
                self.riden_msg = "⚠ choose the supply's serial port".into();
                return;
            }
            match SerialLink::open(&rc.port, rc.baud) {
                Ok(l) => Box::new(l),
                Err(e) => {
                    self.riden_msg = format!("⚠ {e:#}");
                    return;
                }
            }
        };
        let ctx = ctx.clone();
        let notify: Arc<dyn Fn() + Send + Sync> = Arc::new(move || ctx.request_repaint());
        self.detector = BreakDetector::new(rc.break_fraction);
        self.riden = Some(RidenHandle::start(link, rc.address, Some(notify)));
        self.riden_msg = "connecting…".into();
    }

    pub fn disconnect_riden(&mut self) {
        self.riden = None; // drop switches the output off
        self.fake_riden = None;
        self.reading = None;
        self.riden_msg = "disconnected".into();
    }

    /// Handle supply events; pause the cut if the wire looks broken.
    fn poll_riden(&mut self, m: &Machine) {
        let events = self.riden.as_ref().map(|r| r.poll()).unwrap_or_default();
        for e in events {
            match e {
                RidenEvent::Connected { model } => {
                    self.riden_msg = format!("connected: {model}");
                    self.riden_model = model;
                }
                RidenEvent::Reading(r) => {
                    self.reading = Some(r);
                    let cutting = self.job.as_ref().is_some_and(|j| !j.paused);
                    let wire_on = self
                        .status
                        .as_ref()
                        .and_then(|s| s.spindle)
                        .is_some_and(|s| s > 0.0);
                    if cutting && m.riden.break_detect && self.detector.update(&r, wire_on) {
                        self.send(Command::Pause);
                        self.riden_off();
                        self.message = format!(
                            "⚠ wire current dropped to {:.2} A (set {:.2} A): broken wire or lost contact. Cut paused, wire off. Check the wire, then Resume or STOP.",
                            r.i_out, r.i_set
                        );
                        let msg = self.message.clone();
                        self.log(msg);
                    }
                }
                RidenEvent::Error(e) => self.riden_msg = format!("⚠ {e}"),
                RidenEvent::Disconnected(why) => {
                    self.riden_msg = format!("⚠ supply disconnected: {why}");
                    self.riden = None;
                    self.reading = None;
                }
            }
        }
    }

    pub fn connect(&mut self, m: &Machine) {
        let (tx, rx) = mpsc::channel();
        let m = m.clone();
        std::thread::spawn(move || {
            let _ = tx.send(open_link(&m));
        });
        self.connecting = Some(rx);
        self.message = "connecting…".into();
    }

    pub fn disconnect(&mut self) {
        self.riden_off();
        self.ctl = None; // drop: stops any job and closes the link
        self.sim = None;
        self.status = None;
        self.mpos = None;
        self.job = None;
        self.homed = false;
        self.message = "disconnected".into();
    }

    /// Handle finished connection attempts and controller events.
    pub fn poll(&mut self, ctx: &egui::Context, m: &Machine) -> bool {
        let mut job_ended = false;
        if let Some(rx) = &self.connecting {
            match rx.try_recv() {
                Ok(Ok((link, sim))) => {
                    self.sim = sim;
                    let ctx = ctx.clone();
                    let notify: Arc<dyn Fn() + Send + Sync> =
                        Arc::new(move || ctx.request_repaint());
                    let opts = Options {
                        rx_buffer: m.connection.rx_buffer,
                        ..Options::default()
                    };
                    let ctl = Controller::start(link, opts, Some(notify));
                    self.message = format!("connected: {}", ctl.describe);
                    self.log(format!("— connected to {}", ctl.describe));
                    self.ctl = Some(ctl);
                    self.connecting = None;
                }
                Ok(Err(e)) => {
                    self.message = format!("⚠ {e}");
                    self.connecting = None;
                }
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100))
                }
                Err(TryRecvError::Disconnected) => self.connecting = None,
            }
        }
        self.poll_riden(m);
        let events = self.ctl.as_ref().map(|c| c.poll()).unwrap_or_default();
        for e in events {
            match e {
                Event::Status { status, mpos } => {
                    if mpos.is_some() {
                        self.mpos = mpos;
                    }
                    self.status = Some(status);
                }
                Event::Console(t) => {
                    if t.contains("Homed") {
                        self.homed = true;
                    }
                    if t.starts_with("Grbl") {
                        self.homed = false;
                    }
                    self.log(t);
                }
                Event::Error(t) => {
                    self.message = format!("⚠ {t}");
                    self.log(format!("⚠ {t}"));
                }
                Event::Alarm(t) => {
                    self.homed = false;
                    self.message = format!("⚠ {t}");
                    self.log(format!("⚠ {t}"));
                }
                Event::JobStarted {
                    name,
                    total_lines,
                    total_secs,
                } => {
                    self.log(format!("— started {name} ({total_lines} lines)"));
                    self.job = Some(JobView {
                        name,
                        total: total_lines,
                        acked: 0,
                        total_secs,
                        remaining_secs: total_secs,
                        paused: false,
                    });
                }
                Event::Progress {
                    acked,
                    remaining_secs,
                    ..
                } => {
                    if let Some(j) = &mut self.job {
                        j.acked = acked;
                        j.remaining_secs = remaining_secs;
                    }
                }
                Event::Paused => {
                    if let Some(j) = &mut self.job {
                        j.paused = true;
                    }
                    self.log("— paused (wire off)");
                }
                Event::Resumed => {
                    if let Some(j) = &mut self.job {
                        j.paused = false;
                    }
                    self.log("— resumed");
                }
                Event::JobFinished { name, secs } => {
                    self.message = format!(
                        "{name} finished in {}",
                        wingcut::gcode::fmt_minutes(secs / 60.0)
                    );
                    self.log(format!("— {}", self.message));
                    self.job = None;
                    job_ended = true;
                }
                Event::JobStopped { name, reason } => {
                    self.message = format!("{name} {reason}; wire off");
                    self.log(format!("— {}", self.message));
                    self.job = None;
                    job_ended = true;
                }
                Event::JobFailed { name, reason } => {
                    self.message = format!("⚠ {name} failed: {reason}");
                    self.log(self.message.clone());
                    self.job = None;
                    job_ended = true;
                }
                Event::Disconnected(why) => {
                    self.log(format!("— disconnected: {why}"));
                    self.ctl = None;
                    self.status = None;
                    self.job = None;
                    self.message = format!("⚠ disconnected: {why}");
                    job_ended = true;
                }
            }
        }
        if job_ended {
            // Extra safety on top of the board's M5 / reset.
            self.riden_off();
        }
        job_ended
    }

    /// Build the selected panel's job and the pre-flight checks.
    pub fn prepare(
        &mut self,
        app_machine: &Machine,
        plan: Result<(CutPlan, String, Option<f64>), String>,
    ) {
        let (plan, gcode, panel_current) = match plan {
            Ok(p) => p,
            Err(e) => {
                self.message = format!("⚠ cannot run: {e}");
                return;
            }
        };
        let job = Job::from_gcode(
            &plan.panel,
            &gcode,
            axes(app_machine),
            app_machine.feed_rapid,
            self.dry_run,
        );
        let mut checks = Vec::new();
        let state = self.status.as_ref().map(|s| s.state.clone());
        checks.push(match &state {
            Some(State::Idle) => (Check::Ok, "machine idle".to_string()),
            Some(s) => (
                Check::Block,
                format!("machine is {}: it must be Idle", s.name()),
            ),
            None => (Check::Block, "no status from the machine yet".to_string()),
        });
        checks.push(if self.homed {
            (Check::Ok, "homed this session".to_string())
        } else {
            (Check::Warn, "not homed this session: positions are only right if the machine was homed or zeroed by hand".to_string())
        });
        let start = [plan.root_tower[0], plan.tip_tower[0]];
        match self.mpos.as_deref().and_then(|p| towers(app_machine, p)) {
            Some([r, t]) => {
                let front = r[0] <= start[0].x + 0.5 && t[0] <= start[1].x + 0.5;
                checks.push(if front {
                    (Check::Ok, format!("wire in front of the block (root x {:.1}, tip x {:.1})", r[0], t[0]))
                } else {
                    (
                        Check::Block,
                        format!(
                            "wire is not in front of the block: jog both towers to x ≤ {:.1} / {:.1} first (now {:.1} / {:.1}); the first move is vertical",
                            start[0].x, start[1].x, r[0], t[0]
                        ),
                    )
                });
            }
            None => checks.push((Check::Block, "machine position unknown".into())),
        }
        if let Some(w) = self.status.as_ref().and_then(|s| s.wco.as_ref())
            && w.iter().any(|v| v.abs() > 1e-6)
        {
            checks.push((
                Check::Warn,
                format!(
                    "work offset is not zero ({w:?}): wingcut programs use machine coordinates"
                ),
            ));
        }
        checks.push(if self.dry_run {
            (Check::Ok, "dry run: the wire stays cold".into())
        } else {
            (Check::Warn, "the wire will heat: set the current, keep the E-stop in reach, never leave it unattended".into())
        });
        checks.push((
            Check::Ok,
            format!(
                "{}: {} lines, about {}",
                plan.panel,
                job.lines.len(),
                wingcut::gcode::fmt_minutes(job.total_secs / 60.0)
            ),
        ));
        let rc = &app_machine.riden;
        let riden = if !rc.enabled {
            checks.push((Check::Ok, "wire current: set on the supply by hand".into()));
            None
        } else if self.dry_run {
            checks.push((Check::Ok, "dry run: the Riden is left alone".into()));
            None
        } else if self.riden.is_some() && self.reading.is_some() {
            let amps = panel_current.unwrap_or(rc.current);
            let from = if panel_current.is_some() {
                "panel"
            } else {
                "machine"
            };
            checks.push((
                Check::Ok,
                format!(
                    "wire current {amps:.2} A ({from} setting), limit {:.0} V: set on the {} at start{}",
                    rc.max_voltage,
                    self.riden_model,
                    if rc.break_detect { "; broken-wire detection on" } else { "" }
                ),
            ));
            Some((amps, rc.max_voltage))
        } else {
            checks.push((
                Check::Block,
                "the Riden supply is enabled but not connected: connect it under Wire supply, or untick it".into(),
            ));
            None
        };
        self.preflight = Some(Preflight { job, checks, riden });
    }
}

fn state_color(s: &State) -> Color32 {
    match s {
        State::Idle => Color32::from_rgb(60, 170, 90),
        State::Run | State::Jog | State::Home => Color32::from_rgb(70, 130, 230),
        State::Hold(_) | State::Door(_) => Color32::from_rgb(220, 170, 30),
        State::Alarm => Color32::from_rgb(220, 60, 60),
        _ => Color32::GRAY,
    }
}

/// Plan the selected panel strictly and emit its G-code.
pub fn selected_job(app: &App) -> Result<(CutPlan, String, Option<f64>), String> {
    (|| -> anyhow::Result<(CutPlan, String, Option<f64>)> {
        let m = app.machine.clone().normalized()?;
        let mut w = app.wing.clone();
        w.validate()?;
        let name = app
            .selected_panel()
            .ok_or_else(|| anyhow::anyhow!("no panel selected"))?
            .name
            .clone();
        let model = WingModel::new(w, m.max_segment)?;
        let panel = model.wing.panel(&name)?;
        let plan = plan_panel(&model, &m, panel)?;
        let g = wingcut::gcode::emit(&plan, &m, &model.wing.name);
        Ok((plan, g, panel.wire_current))
    })()
    .map_err(|e| format!("{e:#}"))
}

/// The Control tab. Returns true if the machine settings changed.
pub fn ui(ui: &mut Ui, app: &mut App) -> bool {
    let mut changed = false;
    let busy = app.control.running();
    crate::help::heading(ui, "Connection", "connection");
    let c = &mut app.machine.connection;
    let editable = !app.control.connected();
    ui.add_enabled_ui(editable, |ui| {
        ui.horizontal(|ui| {
            for (k, label, hint) in [
                (LinkKind::Serial, "USB", "USB serial"),
                (
                    LinkKind::Websocket,
                    "Wi-Fi (WebSocket)",
                    "FluidNC's WebUI WebSocket, usually port 81",
                ),
                (
                    LinkKind::Telnet,
                    "Wi-Fi (telnet)",
                    "FluidNC's telnet server, usually port 23",
                ),
                (
                    LinkKind::Simulator,
                    "Simulator",
                    "A simulated board: try everything without hardware",
                ),
            ] {
                changed |= ui
                    .selectable_value(&mut c.kind, k, label)
                    .on_hover_text(hint)
                    .changed();
            }
        });
        egui::Grid::new("conn").num_columns(2).show(ui, |ui| {
            match c.kind {
                LinkKind::Serial => {
                    ui.label("Port");
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("port")
                            .selected_text(if c.port.is_empty() {
                                "choose…"
                            } else {
                                &c.port
                            })
                            .show_ui(ui, |ui| {
                                for p in &app.control.ports {
                                    changed |=
                                        ui.selectable_value(&mut c.port, p.clone(), p).changed();
                                }
                            });
                        if ui
                            .small_button("⟳")
                            .on_hover_text("Refresh the port list")
                            .clicked()
                        {
                            app.control.ports = wingcut_machine::serial_ports();
                        }
                        changed |= ui
                            .add(
                                egui::TextEdit::singleline(&mut c.port)
                                    .desired_width(110.0)
                                    .hint_text("/dev/ttyUSB0"),
                            )
                            .changed();
                    });
                    ui.end_row();
                    ui.label("Baud");
                    changed |= ui
                        .add(egui::DragValue::new(&mut c.baud).speed(100.0))
                        .changed();
                    ui.end_row();
                }
                LinkKind::Websocket | LinkKind::Telnet => {
                    ui.label("Host");
                    changed |= ui.text_edit_singleline(&mut c.host).changed();
                    ui.end_row();
                    ui.label("Port");
                    let p = if c.kind == LinkKind::Websocket {
                        &mut c.websocket_port
                    } else {
                        &mut c.telnet_port
                    };
                    changed |= ui.add(egui::DragValue::new(p)).changed();
                    ui.end_row();
                }
                LinkKind::Simulator => {
                    ui.label("Speed");
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut c.sim_speed)
                                .speed(0.5)
                                .range(0.1..=1000.0)
                                .suffix("× real time"),
                        )
                        .changed();
                    ui.end_row();
                }
            }
            ui.label("RX buffer").on_hover_text(
                "Bytes kept in the board's input buffer while streaming; 127 is safe",
            );
            changed |= ui
                .add(
                    egui::DragValue::new(&mut c.rx_buffer)
                        .range(16..=10_000)
                        .suffix(" bytes"),
                )
                .changed();
            ui.end_row();
        });
    });
    let machine = app.machine.clone();
    ui.horizontal(|ui| {
        if app.control.connected() {
            if ui
                .add_enabled(!busy, egui::Button::new("Disconnect"))
                .clicked()
            {
                app.control.disconnect();
            }
        } else if ui
            .add_enabled(
                app.control.connecting.is_none(),
                egui::Button::new("Connect"),
            )
            .clicked()
        {
            app.control.connect(&machine);
        }
        let msg = &app.control.message;
        let col = if msg.starts_with('⚠') {
            Color32::from_rgb(220, 80, 60)
        } else {
            ui.visuals().weak_text_color()
        };
        // Truncated so long messages never widen the panel; full text on
        // hover and in the console.
        ui.add(egui::Label::new(RichText::new(msg).color(col)).truncate())
            .on_hover_text(msg);
    });
    changed |= riden_ui(ui, app);
    if !app.control.connected() {
        if cfg!(target_os = "linux") && machine.connection.kind == LinkKind::Serial {
            ui.label(
                RichText::new("Linux: your user must be in the 'dialout' group for USB serial.")
                    .small()
                    .weak(),
            );
        }
        return changed;
    }

    ui.separator();
    // State and position.
    let ctl = &mut app.control;
    ui.horizontal(|ui| {
        if let Some(st) = &ctl.status {
            ui.label(
                RichText::new(st.state.name())
                    .strong()
                    .size(18.0)
                    .color(state_color(&st.state)),
            );
            match st.spindle {
                Some(s) if s > 0.0 => {
                    ui.label(
                        RichText::new(" wire ON ")
                            .strong()
                            .color(Color32::WHITE)
                            .background_color(Color32::from_rgb(200, 60, 30)),
                    );
                }
                Some(_) => {
                    ui.label(RichText::new("wire off").weak());
                }
                None => {}
            }
        }
        if let Some([r, t]) = ctl.mpos.as_deref().and_then(|p| towers(&machine, p)) {
            ui.monospace(format!(
                "root {}{:>8.2} {}{:>7.2}   tip {}{:>8.2} {}{:>7.2}",
                machine.axes[0],
                r[0],
                machine.axes[1],
                r[1],
                machine.axes[2],
                t[0],
                machine.axes[3],
                t[1]
            ));
        }
    });
    ui.horizontal(|ui| {
        ui.add_enabled_ui(!busy, |ui| {
            if ui
                .button("⌂ Home")
                .on_hover_text("$H: home all axes")
                .clicked()
            {
                ctl.home();
            }
            if ui
                .button("Unlock")
                .on_hover_text("$X: clear an alarm without homing (positions may be wrong)")
                .clicked()
            {
                ctl.send(Command::Unlock);
            }
        });
        if ui
            .button("Reset")
            .on_hover_text("Soft reset: stops everything, wire off")
            .clicked()
        {
            ctl.send(Command::Reset);
        }
        if !ctl.homed {
            ui.label(RichText::new("not homed").color(Color32::from_rgb(220, 170, 30)));
        }
    });

    // Jog.
    ui.add_space(4.0);
    crate::help::label(ui, "Jog", "jog", "Move the wire by hand");
    ui.add_enabled_ui(!busy, |ui| {
        ui.horizontal(|ui| {
            for (m, l) in [
                (JogMode::Both, "both towers"),
                (JogMode::Root, "root only"),
                (JogMode::Tip, "tip only"),
            ] {
                ui.selectable_value(&mut ctl.mode, m, l);
            }
        });
        ui.horizontal(|ui| {
            ui.label("step");
            for s in [0.1, 1.0, 10.0, 50.0, 100.0] {
                ui.selectable_value(&mut ctl.step, s, format!("{s}"));
            }
            ui.label("feed");
            ui.add(
                egui::DragValue::new(&mut ctl.feed)
                    .speed(10.0)
                    .range(10.0..=5000.0)
                    .suffix(" mm/min"),
            );
        });
        let a = axes(&machine);
        let (xs, ys): (Vec<char>, Vec<char>) = match ctl.mode {
            JogMode::Both => (vec![a[0], a[2]], vec![a[1], a[3]]),
            JogMode::Root => (vec![a[0]], vec![a[1]]),
            JogMode::Tip => (vec![a[2]], vec![a[3]]),
        };
        let jog = |ctl: &Control, letters: &[char], d: f64| {
            let moves: Vec<(char, f64)> = letters.iter().map(|&l| (l, d)).collect();
            ctl.jog(&moves, ctl.feed);
        };
        egui::Grid::new("jogpad").show(ui, |ui| {
            ui.label("");
            if ui.button("  ▲ up  ").clicked() {
                jog(ctl, &ys, ctl.step);
            }
            ui.label("");
            ui.end_row();
            if ui
                .button("◀ to front")
                .on_hover_text("Towards the operator (−x)")
                .clicked()
            {
                jog(ctl, &xs, -ctl.step);
            }
            if ui
                .button("■ stop")
                .on_hover_text("Cancel the jog")
                .clicked()
            {
                ctl.send(Command::JogCancel);
            }
            if ui
                .button("to back ▶")
                .on_hover_text("Away from the operator (+x)")
                .clicked()
            {
                jog(ctl, &xs, ctl.step);
            }
            ui.end_row();
            ui.label("");
            if ui.button(" ▼ down ").clicked() {
                jog(ctl, &ys, -ctl.step);
            }
            ui.end_row();
        });
    });

    // Cut.
    ui.add_space(4.0);
    crate::help::label(ui, "Cut", "running-a-cut", "");
    let panel_name = app
        .selected_panel()
        .map(|p| p.name.clone())
        .unwrap_or_default();
    if let Some(j) = &app.control.job {
        let frac = if j.total > 0 {
            j.acked as f32 / j.total as f32
        } else {
            0.0
        };
        ui.add(ProgressBar::new(frac).text(format!(
            "{}: line {}/{}, about {} of {} left{}",
            j.name,
            j.acked,
            j.total,
            wingcut::gcode::fmt_minutes(j.remaining_secs / 60.0),
            wingcut::gcode::fmt_minutes(j.total_secs / 60.0),
            if j.paused { " (paused, wire off)" } else { "" }
        )));
        let paused = j.paused;
        ui.horizontal(|ui| {
            if paused {
                if ui.button("▶ Resume").clicked() {
                    app.control.resume();
                }
            } else if ui
                .button("Pause")
                .on_hover_text("Feed hold, then wire off")
                .clicked()
            {
                app.control.send(Command::Pause);
            }
            let stop = egui::Button::new(RichText::new("■ STOP").strong().color(Color32::WHITE))
                .fill(Color32::from_rgb(190, 40, 40));
            if ui
                .add(stop)
                .on_hover_text("Hold, then reset: stops the cut and switches the wire off")
                .clicked()
            {
                app.control.stop();
            }
        });
    } else {
        ui.checkbox(&mut app.control.dry_run, "dry run (wire stays cold)");
        if ui
            .add_enabled(
                !panel_name.is_empty(),
                egui::Button::new(format!("▶ Run panel {panel_name}…")),
            )
            .clicked()
        {
            let job = selected_job(app);
            app.control.prepare(&machine, job);
        }
    }

    // Console.
    ui.add_space(4.0);
    crate::help::label(
        ui,
        "Console",
        "console",
        "Everything the board says; type commands below",
    );
    egui::ScrollArea::vertical()
        .max_height(160.0)
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for l in &app.control.log {
                ui.monospace(l);
            }
        });
    ui.horizontal(|ui| {
        let r = ui.add_enabled(
            !busy,
            egui::TextEdit::singleline(&mut app.control.input)
                .hint_text("G-code or $ command")
                .desired_width(260.0),
        );
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (ui.add_enabled(!busy, egui::Button::new("Send")).clicked() || enter)
            && !app.control.input.trim().is_empty()
        {
            let line = std::mem::take(&mut app.control.input);
            app.control.log(format!("> {line}"));
            app.control.send(Command::Send(line));
        }
    });
    changed
}

/// "Wire supply (Riden)" section. Returns true if settings changed.
fn riden_ui(ui: &mut Ui, app: &mut App) -> bool {
    let mut changed = false;
    ui.separator();
    crate::help::heading(ui, "Wire supply (Riden)", "wire-supply-riden");
    let busy = app.control.running();
    let connected = app.control.riden.is_some();
    let rc = &mut app.machine.riden;
    changed |= ui
        .add_enabled(!connected, egui::Checkbox::new(&mut rc.enabled, "let wingcut set the wire current and watch the wire"))
        .on_hover_text("RD6006/RD6012/RD6018/RD6024 over its USB port (Modbus). Off: you set the current on the supply by hand.")
        .changed();
    if !rc.enabled {
        return changed;
    }
    ui.add_enabled_ui(!connected, |ui| {
        egui::Grid::new("riden").num_columns(2).show(ui, |ui| {
            ui.label("Supply");
            ui.horizontal(|ui| {
                changed |= ui
                    .selectable_value(&mut rc.simulated, false, "USB")
                    .changed();
                changed |= ui
                    .selectable_value(&mut rc.simulated, true, "simulated")
                    .changed();
            });
            ui.end_row();
            if !rc.simulated {
                ui.label("Port");
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("riden-port")
                        .selected_text(if rc.port.is_empty() {
                            "choose…"
                        } else {
                            &rc.port
                        })
                        .show_ui(ui, |ui| {
                            for p in &app.control.ports {
                                changed |=
                                    ui.selectable_value(&mut rc.port, p.clone(), p).changed();
                            }
                        });
                    changed |= ui
                        .add(
                            egui::TextEdit::singleline(&mut rc.port)
                                .desired_width(110.0)
                                .hint_text("/dev/ttyUSB1"),
                        )
                        .changed();
                });
                ui.end_row();
                ui.label("Baud / address");
                ui.horizontal(|ui| {
                    changed |= ui
                        .add(egui::DragValue::new(&mut rc.baud).speed(100.0))
                        .changed();
                    changed |= ui
                        .add(egui::DragValue::new(&mut rc.address).range(1..=247))
                        .changed();
                });
                ui.end_row();
            }
        });
    });
    egui::Grid::new("riden-set").num_columns(2).show(ui, |ui| {
        ui.label("Wire current").on_hover_text("Default for every panel; a panel can set its own");
        changed |= ui
            .add(egui::DragValue::new(&mut rc.current).speed(0.01).range(0.05..=24.0).suffix(" A").max_decimals(3))
            .changed();
        ui.end_row();
        ui.label("Voltage limit");
        changed |= ui.add(egui::DragValue::new(&mut rc.max_voltage).speed(0.5).range(1.0..=60.0).suffix(" V")).changed();
        ui.end_row();
        ui.label("Broken-wire check").on_hover_text("Pause the cut and switch the wire off if the current collapses while the wire should be on");
        ui.horizontal(|ui| {
            changed |= ui.checkbox(&mut rc.break_detect, "below").changed();
            changed |= ui
                .add(egui::DragValue::new(&mut rc.break_fraction).speed(0.01).range(0.05..=0.95).max_decimals(2))
                .changed();
            ui.label("× set current");
        });
        ui.end_row();
    });
    let machine = app.machine.clone();
    let ctl = &mut app.control;
    ui.horizontal(|ui| {
        if ctl.riden.is_some() {
            if ui
                .add_enabled(!busy, egui::Button::new("Disconnect supply"))
                .clicked()
            {
                ctl.disconnect_riden();
            }
        } else if ui.button("Connect supply").clicked() {
            let ctx = ui.ctx().clone();
            ctl.connect_riden(&ctx, &machine);
        }
        let col = if ctl.riden_msg.starts_with('⚠') {
            Color32::from_rgb(220, 80, 60)
        } else {
            ui.visuals().weak_text_color()
        };
        ui.add(egui::Label::new(RichText::new(&ctl.riden_msg).color(col)).truncate())
            .on_hover_text(&ctl.riden_msg);
    });
    if let Some(r) = ctl.reading {
        ui.horizontal(|ui| {
            ui.monospace(format!(
                "{:5.2} V  {:5.3} A (set {:.3} A){}",
                r.v_out,
                r.i_out,
                r.i_set,
                r.resistance()
                    .map(|o| format!("  {o:5.1} Ω"))
                    .unwrap_or_default()
            ));
            if r.output {
                ui.label(
                    RichText::new(" output ON ")
                        .strong()
                        .color(Color32::WHITE)
                        .background_color(Color32::from_rgb(200, 60, 30)),
                );
            } else {
                ui.label(RichText::new("output off").weak());
            }
        });
        ui.horizontal(|ui| {
            ui.add_enabled_ui(!busy, |ui| {
                if ui
                    .button("Apply current")
                    .on_hover_text("Send the current and voltage limit to the supply now")
                    .clicked()
                    && let Some(h) = &ctl.riden
                {
                    h.send(RidenCommand::SetVoltage(machine.riden.max_voltage));
                    h.send(RidenCommand::SetCurrent(machine.riden.current));
                }
                if ui.button("Output off").clicked() {
                    ctl.riden_off();
                }
            });
            if let Some(f) = &ctl.fake_riden {
                let mut f = f.lock().expect("fake lock");
                let broken = f.wire_ohms.is_none();
                if ui
                    .button(if broken {
                        "Mend the wire"
                    } else {
                        "Break the wire"
                    })
                    .on_hover_text("Simulated supply only: test the broken-wire check")
                    .clicked()
                {
                    f.wire_ohms = if broken { Some(15.0) } else { None };
                }
            }
        });
    }
    changed
}

/// The pre-flight dialog; starts the job on confirmation.
pub fn preflight_dialog(ctx: &egui::Context, control: &mut Control) {
    let Some(pf) = &control.preflight else { return };
    let blocked = pf.checks.iter().any(|(c, _)| *c == Check::Block);
    let mut close = false;
    let mut start = false;
    egui::Modal::new(egui::Id::unique("preflight")).show(ctx, |ui| {
        ui.heading("Before cutting");
        for (c, text) in &pf.checks {
            let (icon, col) = match c {
                Check::Ok => ("✔", Color32::from_rgb(60, 170, 90)),
                Check::Warn => ("⚠", Color32::from_rgb(220, 170, 30)),
                Check::Block => ("✖", Color32::from_rgb(220, 60, 60)),
            };
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(icon).color(col));
                ui.label(text);
            });
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !blocked,
                    egui::Button::new(RichText::new("Start cut").strong()),
                )
                .clicked()
            {
                start = true;
            }
            if ui.button("Cancel").clicked() {
                close = true;
            }
        });
    });
    if start {
        control.start_prepared();
    } else if close {
        control.preflight = None;
    }
}
