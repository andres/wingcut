//! Background controller: owns the link, polls status, streams jobs with
//! grbl character counting, and handles pause/resume/stop safely.
//!
//! Safety rules it follows:
//! * Pause = feed hold, then (once stopped) spindle stop, so the wire cools
//!   while held; resume restores it.
//! * Stop / error / alarm = feed hold, then soft reset, which clears the
//!   buffers and switches the wire off. The hardware E-stop stays the
//!   primary safety: if the link dies, the board finishes what it has
//!   buffered.

use crate::grbl::{self, Motion, Reply, State, Status, rt};
use crate::link::Link;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct Options {
    /// Controller RX buffer to fill (bytes). 127 is safe for grbl and FluidNC.
    pub rx_buffer: usize,
    pub status_every: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            rx_buffer: 127,
            status_every: Duration::from_millis(200),
        }
    }
}

/// One program line to send.
#[derive(Debug, Clone, PartialEq)]
pub struct JobLine {
    /// 1-based line number in the original file.
    pub n: usize,
    pub text: String,
    /// Estimated execution time (s).
    pub secs: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub name: String,
    pub lines: Vec<JobLine>,
    pub total_secs: f64,
}

impl Job {
    /// Prepare G-code: strip comments and blank lines, estimate times.
    /// `dry_run` drops every line that switches the wire on (M3/M4), so the
    /// machine moves through the cut cold.
    pub fn from_gcode(name: &str, gcode: &str, axes: [char; 4], rapid: f64, dry_run: bool) -> Job {
        let mut m = Motion::new(axes, rapid);
        let mut lines = Vec::new();
        for (i, raw) in gcode.lines().enumerate() {
            let Some(text) = grbl::clean_line(raw) else {
                continue;
            };
            if dry_run
                && grbl::words(&text)
                    .iter()
                    .any(|&(c, v)| c == 'M' && (v as u32 == 3 || v as u32 == 4))
            {
                continue;
            }
            let (secs, _) = m.apply(&text);
            lines.push(JobLine {
                n: i + 1,
                text,
                secs,
            });
        }
        let total_secs = lines.iter().map(|l| l.secs).sum();
        Job {
            name: name.to_string(),
            lines,
            total_secs,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Send one line (not while a job runs).
    Send(String),
    Start(Job),
    Pause,
    Resume,
    Stop,
    Home,
    Unlock,
    Reset,
    /// Relative jog: (axis letter, distance mm) pairs at `feed` mm/min.
    Jog {
        moves: Vec<(char, f64)>,
        feed: f64,
    },
    JogCancel,
    Disconnect,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Status {
        status: Status,
        mpos: Option<Vec<f64>>,
    },
    /// Anything the board printed that isn't ok/status.
    Console(String),
    /// A manual command failed.
    Error(String),
    Alarm(String),
    JobStarted {
        name: String,
        total_lines: usize,
        total_secs: f64,
    },
    Progress {
        acked: usize,
        total: usize,
        remaining_secs: f64,
    },
    Paused,
    Resumed,
    JobFinished {
        name: String,
        secs: f64,
    },
    JobStopped {
        name: String,
        reason: String,
    },
    JobFailed {
        name: String,
        reason: String,
    },
    Disconnected(String),
}

/// Handle to the background controller thread.
pub struct Controller {
    tx: Sender<Command>,
    rx: Receiver<Event>,
    thread: Option<JoinHandle<()>>,
    pub describe: String,
}

impl Controller {
    /// Start the controller on `link`. `notify` is called whenever events
    /// are waiting (e.g. to repaint a GUI).
    pub fn start(
        link: Box<dyn Link>,
        opts: Options,
        notify: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Controller {
        let (tx, cmd_rx) = mpsc::channel();
        let (ev_tx, rx) = mpsc::channel();
        let describe = link.describe();
        let thread = std::thread::Builder::new()
            .name("wingcut-controller".into())
            .spawn(move || {
                let mut w = Worker::new(link, opts, ev_tx, notify);
                w.run(cmd_rx);
            })
            .expect("spawn controller thread");
        Controller {
            tx,
            rx,
            thread: Some(thread),
            describe,
        }
    }

    pub fn send(&self, cmd: Command) {
        let _ = self.tx.send(cmd);
    }

    /// Events received since the last call.
    pub fn poll(&self) -> Vec<Event> {
        self.rx.try_iter().collect()
    }

    /// Wait up to `timeout` for the next event (for tests and the CLI).
    pub fn next(&self, timeout: Duration) -> Option<Event> {
        self.rx.recv_timeout(timeout).ok()
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Disconnect);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Inflight {
    len: usize,
    /// Index into the job's lines, or `None` for a manual command.
    job_idx: Option<usize>,
    text: String,
}

struct Running {
    job: Job,
    next: usize,
    acked: usize,
    started: Instant,
    paused: bool,
    /// Send spindle stop once the hold has completed.
    wire_off_pending: bool,
    last_progress: Instant,
}

enum Outcome {
    Stopped(String),
    Failed(String),
}

enum Stopping {
    /// Feed hold sent; waiting for the machine to stand still.
    Hold { since: Instant, outcome: Outcome },
    /// Soft reset sent; waiting for the banner.
    Reset { since: Instant, outcome: Outcome },
}

struct Worker {
    link: Box<dyn Link>,
    opts: Options,
    events: Sender<Event>,
    notify: Option<Arc<dyn Fn() + Send + Sync>>,
    inflight: VecDeque<Inflight>,
    rx_used: usize,
    manual: VecDeque<String>,
    job: Option<Running>,
    stopping: Option<Stopping>,
    last_status_req: Instant,
    state: Option<State>,
    wco: Option<Vec<f64>>,
    partial: Vec<u8>,
}

type LinkResult = std::io::Result<()>;

impl Worker {
    fn new(
        link: Box<dyn Link>,
        opts: Options,
        events: Sender<Event>,
        notify: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Worker {
        Worker {
            link,
            opts,
            events,
            notify,
            inflight: VecDeque::new(),
            rx_used: 0,
            manual: VecDeque::new(),
            job: None,
            stopping: None,
            last_status_req: Instant::now() - Duration::from_secs(10),
            state: None,
            wco: None,
            partial: Vec::new(),
        }
    }

    fn emit(&self, e: Event) {
        let _ = self.events.send(e);
        if let Some(n) = &self.notify {
            n();
        }
    }

    fn run(&mut self, cmds: Receiver<Command>) {
        let res = (|| -> LinkResult {
            // Wake the board and ask for its state.
            self.link.send(b"\n")?;
            loop {
                loop {
                    match cmds.try_recv() {
                        Ok(Command::Disconnect) | Err(TryRecvError::Disconnected) => {
                            return self.shutdown();
                        }
                        Ok(c) => self.command(c)?,
                        Err(TryRecvError::Empty) => break,
                    }
                }
                self.pump()?;
                if self.last_status_req.elapsed() >= self.opts.status_every {
                    self.link.send(&[rt::STATUS])?;
                    self.last_status_req = Instant::now();
                }
                let mut buf = [0u8; 1024];
                let n = self.link.recv(&mut buf, Duration::from_millis(10))?;
                self.partial.extend_from_slice(&buf[..n]);
                while let Some(i) = self.partial.iter().position(|&b| b == b'\n') {
                    let line: Vec<u8> = self.partial.drain(..=i).collect();
                    let text = String::from_utf8_lossy(&line).trim().to_string();
                    if !text.is_empty() {
                        self.reply(&text)?;
                    }
                }
                self.check_stopping()?;
            }
        })();
        if let Err(e) = res {
            if let Some(r) = self.job.take() {
                self.emit(Event::JobFailed {
                    name: r.job.name,
                    reason: format!(
                        "connection lost ({e}). The board finishes the moves it has buffered and the wire may still be on: use the E-stop if needed."
                    ),
                });
            }
            self.emit(Event::Disconnected(e.to_string()));
        } else {
            self.emit(Event::Disconnected("disconnected".into()));
        }
    }

    /// On disconnect: stop any job (hold + reset switches the wire off).
    fn shutdown(&mut self) -> LinkResult {
        if self.job.is_some() {
            self.link.send(&[rt::FEED_HOLD])?;
            std::thread::sleep(Duration::from_millis(300));
            self.link.send(&[rt::SOFT_RESET])?;
            if let Some(r) = self.job.take() {
                self.emit(Event::JobStopped {
                    name: r.job.name,
                    reason: "disconnected".into(),
                });
            }
        }
        Ok(())
    }

    fn busy(&self) -> bool {
        self.job.is_some() || self.stopping.is_some()
    }

    fn manual(&mut self, line: String) {
        if self.busy() {
            self.emit(Event::Error(format!("'{line}' not sent: a job is running")));
        } else {
            self.manual.push_back(line);
        }
    }

    fn command(&mut self, c: Command) -> LinkResult {
        match c {
            Command::Send(l) => {
                if let Some(t) = grbl::clean_line(&l) {
                    self.manual(t);
                }
            }
            Command::Home => self.manual("$H".into()),
            Command::Unlock => self.manual("$X".into()),
            Command::Jog { moves, feed } => {
                let axes: String = moves.iter().map(|(a, d)| format!(" {a}{d:.3}")).collect();
                self.manual(format!("$J=G91 G21{axes} F{feed:.0}"));
            }
            Command::JogCancel => self.link.send(&[rt::JOG_CANCEL])?,
            Command::Reset => {
                self.link.send(&[rt::SOFT_RESET])?;
                self.clear_buffers();
            }
            Command::Start(job) => {
                if self.busy() {
                    self.emit(Event::Error("a job is already running".into()));
                } else if matches!(self.state, Some(State::Alarm)) {
                    self.emit(Event::Error(
                        "the machine is in alarm: home ($H) or unlock ($X) first".into(),
                    ));
                } else if job.lines.is_empty() {
                    self.emit(Event::Error("the job is empty".into()));
                } else {
                    self.emit(Event::JobStarted {
                        name: job.name.clone(),
                        total_lines: job.lines.len(),
                        total_secs: job.total_secs,
                    });
                    self.job = Some(Running {
                        job,
                        next: 0,
                        acked: 0,
                        started: Instant::now(),
                        paused: false,
                        wire_off_pending: false,
                        last_progress: Instant::now(),
                    });
                }
            }
            Command::Pause => {
                if let Some(r) = self.job.as_mut()
                    && !r.paused
                {
                    self.link.send(&[rt::FEED_HOLD])?;
                    let r = self.job.as_mut().expect("checked");
                    r.paused = true;
                    r.wire_off_pending = true;
                    self.emit(Event::Paused);
                }
            }
            Command::Resume => {
                if let Some(r) = self.job.as_mut()
                    && r.paused
                {
                    r.paused = false;
                    r.wire_off_pending = false;
                    self.link.send(&[rt::CYCLE_START])?;
                    self.emit(Event::Resumed);
                }
            }
            Command::Stop => self.begin_stop(Outcome::Stopped("stopped".into()))?,
            Command::Disconnect => {}
        }
        Ok(())
    }

    fn begin_stop(&mut self, outcome: Outcome) -> LinkResult {
        if self.stopping.is_some() {
            return Ok(());
        }
        self.link.send(&[rt::FEED_HOLD])?;
        self.stopping = Some(Stopping::Hold {
            since: Instant::now(),
            outcome,
        });
        Ok(())
    }

    fn check_stopping(&mut self) -> LinkResult {
        let standing = matches!(
            self.state,
            Some(State::Hold(0) | State::Idle | State::Alarm)
        );
        match self.stopping.take() {
            Some(Stopping::Hold { since, outcome })
                if standing || since.elapsed() > Duration::from_secs(3) =>
            {
                self.link.send(&[rt::SOFT_RESET])?;
                self.clear_buffers();
                self.stopping = Some(Stopping::Reset {
                    since: Instant::now(),
                    outcome,
                });
            }
            Some(Stopping::Reset { since, outcome })
                if since.elapsed() > Duration::from_secs(2) =>
            {
                self.finish_stop(outcome);
            }
            other => self.stopping = other,
        }
        Ok(())
    }

    fn finish_stop(&mut self, outcome: Outcome) {
        let name = self.job.take().map(|r| r.job.name).unwrap_or_default();
        self.emit(match outcome {
            Outcome::Stopped(reason) => Event::JobStopped { name, reason },
            Outcome::Failed(reason) => Event::JobFailed { name, reason },
        });
    }

    fn clear_buffers(&mut self) {
        self.inflight.clear();
        self.rx_used = 0;
        self.manual.clear();
    }

    /// Send manual and job lines while they fit in the controller's RX buffer.
    fn pump(&mut self) -> LinkResult {
        while let Some(l) = self.manual.front() {
            if !self.fits(l) {
                break;
            }
            let l = self.manual.pop_front().expect("front exists");
            self.send_line(l, None)?;
        }
        if self.stopping.is_some() {
            return Ok(());
        }
        while let Some(r) = self.job.as_ref() {
            if r.paused || r.next >= r.job.lines.len() {
                break;
            }
            let text = r.job.lines[r.next].text.clone();
            if !self.fits(&text) {
                break;
            }
            let idx = r.next;
            self.job.as_mut().expect("checked").next += 1;
            self.send_line(text, Some(idx))?;
        }
        Ok(())
    }

    fn fits(&self, line: &str) -> bool {
        // Always allow one line when nothing is in flight (very long lines).
        self.inflight.is_empty() || self.rx_used + line.len() < self.opts.rx_buffer
    }

    fn send_line(&mut self, text: String, job_idx: Option<usize>) -> LinkResult {
        let mut bytes = text.clone().into_bytes();
        bytes.push(b'\n');
        self.link.send(&bytes)?;
        self.rx_used += bytes.len();
        self.inflight.push_back(Inflight {
            len: bytes.len(),
            job_idx,
            text,
        });
        Ok(())
    }

    fn reply(&mut self, line: &str) -> LinkResult {
        match grbl::parse_line(line) {
            Reply::Status(st) => {
                if let Some(w) = &st.wco {
                    self.wco = Some(w.clone());
                }
                let mpos = st.machine_pos(self.wco.as_deref());
                self.state = Some(st.state.clone());
                self.on_state()?;
                self.emit(Event::Status { status: st, mpos });
            }
            Reply::Ok => {
                if let Some(f) = self.inflight.pop_front() {
                    self.rx_used = self.rx_used.saturating_sub(f.len);
                    if f.job_idx.is_some() {
                        self.acked();
                    }
                }
            }
            Reply::Error(code) => {
                let f = self.inflight.pop_front();
                if let Some(f) = &f {
                    self.rx_used = self.rx_used.saturating_sub(f.len);
                }
                let what = format!("error:{code} ({})", grbl::error_text(code));
                match f {
                    Some(Inflight {
                        job_idx: Some(i),
                        text,
                        ..
                    }) => {
                        let n = self.job.as_ref().map_or(0, |r| r.job.lines[i].n);
                        self.begin_stop(Outcome::Failed(format!("line {n} '{text}': {what}")))?;
                    }
                    Some(Inflight { text, .. }) => {
                        self.emit(Event::Error(format!("'{text}': {what}")))
                    }
                    None => self.emit(Event::Error(what)),
                }
            }
            Reply::Alarm(code) => {
                let what = format!("ALARM:{code} ({})", grbl::alarm_text(code));
                self.emit(Event::Alarm(what.clone()));
                if self.job.is_some() {
                    // Motion already stopped; reset clears it and the wire.
                    self.link.send(&[rt::SOFT_RESET])?;
                    self.clear_buffers();
                    self.stopping = Some(Stopping::Reset {
                        since: Instant::now(),
                        outcome: Outcome::Failed(what),
                    });
                }
            }
            Reply::Banner(b) => {
                // The board (re)started: anything in flight is gone.
                self.clear_buffers();
                self.emit(Event::Console(b));
                if let Some(Stopping::Reset { outcome, .. }) = self.stopping.take() {
                    self.finish_stop(outcome);
                } else if let Some(r) = self.job.take() {
                    self.emit(Event::JobFailed {
                        name: r.job.name,
                        reason: "the board reset during the job".into(),
                    });
                }
            }
            Reply::Feedback(t) | Reply::Other(t) => self.emit(Event::Console(t)),
        }
        Ok(())
    }

    fn acked(&mut self) {
        let Some(r) = self.job.as_mut() else { return };
        r.acked += 1;
        let total = r.job.lines.len();
        if r.last_progress.elapsed() > Duration::from_millis(200) || r.acked == total {
            r.last_progress = Instant::now();
            let remaining_secs = r.job.lines[r.acked.min(total)..]
                .iter()
                .map(|l| l.secs)
                .sum();
            let e = Event::Progress {
                acked: r.acked,
                total,
                remaining_secs,
            };
            self.emit(e);
        }
    }

    /// React to a new machine state: finish jobs, cut the wire in a hold.
    fn on_state(&mut self) -> LinkResult {
        let state = self.state.clone();
        let Some(r) = self.job.as_mut() else {
            return Ok(());
        };
        if r.paused && r.wire_off_pending && state == Some(State::Hold(0)) {
            r.wire_off_pending = false;
            self.link.send(&[rt::SPINDLE_STOP])?;
            return Ok(());
        }
        let all_sent = r.acked == r.job.lines.len();
        if all_sent && self.stopping.is_none() && state == Some(State::Idle) {
            let r = self.job.take().expect("checked");
            self.emit(Event::JobFinished {
                name: r.job.name,
                secs: r.started.elapsed().as_secs_f64(),
            });
        }
        Ok(())
    }
}
