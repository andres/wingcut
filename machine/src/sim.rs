//! A simulated FluidNC board, for testing and for trying the machine panel
//! without hardware. It models what matters for safe streaming: the RX
//! buffer (and flags overflows), the planner queue, motion timing (G93/G94,
//! rapids, dwells), feed hold/resume, soft reset, jogging, homing, soft
//! limits and the wire (spindle) state.

use crate::grbl::{Motion, rt, words};
use crate::link::Link;
use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const BANNER: &str = "\r\nGrbl 3.9 [FluidNC v4.1.1 (wingcut simulator) '$' for help]\r\n";

#[derive(Debug, Clone)]
pub struct SimConfig {
    /// Axis letters: root x, root y, tip x, tip y.
    pub axes: [char; 4],
    /// Travel per axis (min, max), for soft limits.
    pub limits: [[f64; 2]; 4],
    pub rapid: f64,
    /// RX buffer size in bytes.
    pub rx_size: usize,
    /// Planner queue length (blocks).
    pub planner: usize,
    /// Simulated seconds per real second.
    pub speed: f64,
    /// Start in alarm until homed or unlocked (like `$22` homing enabled).
    pub require_homing: bool,
    pub home: [f64; 4],
    pub homing_secs: f64,
    pub start: [f64; 4],
}

impl Default for SimConfig {
    fn default() -> Self {
        SimConfig {
            axes: ['X', 'Y', 'Z', 'A'],
            limits: [[0.0, 1000.0], [0.0, 300.0], [0.0, 1000.0], [0.0, 300.0]],
            rapid: 1000.0,
            rx_size: 127,
            planner: 16,
            speed: 1.0,
            require_homing: false,
            home: [0.0; 4],
            homing_secs: 3.0,
            start: [0.0; 4],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Block {
    Move { to: [f64; 4], secs: f64, jog: bool },
    Dwell { secs: f64 },
    Wire(bool),
    Home { secs: f64 },
}

impl Block {
    fn secs(&self) -> f64 {
        match *self {
            Block::Move { secs, .. } | Block::Dwell { secs } | Block::Home { secs } => secs,
            Block::Wire(_) => 0.0,
        }
    }
}

/// Simulator state. Shared behind a mutex so tests can inspect it.
pub struct Sim {
    pub cfg: SimConfig,
    clock: f64,
    last: Instant,
    rx: Vec<u8>,
    out: VecDeque<u8>,
    queue: VecDeque<Block>,
    /// Current block, time spent in it, and where it started.
    cur: Option<(Block, f64, [f64; 4])>,
    pos: [f64; 4],
    motion: Motion,
    alarm: bool,
    hold: bool,
    wire: bool,
    wire_stopped: bool,
    homed: bool,
    lines: usize,
    /// For tests: the most RX bytes ever waiting, and an event log.
    pub max_rx: usize,
    pub log: Vec<String>,
    /// For tests: answer `error:20` to this line number (1-based).
    pub fail_on_line: Option<usize>,
    /// For tests: drop the connection at this simulated time.
    pub disconnect_at: Option<f64>,
}

impl Sim {
    pub fn new(cfg: SimConfig) -> Sim {
        let mut s = Sim {
            clock: 0.0,
            last: Instant::now(),
            rx: Vec::new(),
            out: VecDeque::new(),
            queue: VecDeque::new(),
            cur: None,
            pos: cfg.start,
            motion: Motion::new(cfg.axes, cfg.rapid),
            alarm: cfg.require_homing,
            hold: false,
            wire: false,
            wire_stopped: false,
            homed: false,
            lines: 0,
            max_rx: 0,
            log: Vec::new(),
            fail_on_line: None,
            disconnect_at: None,
            cfg,
        };
        s.motion.pos = s.pos;
        s.say(BANNER.trim_start_matches("\r\n"));
        if s.alarm {
            s.say("[MSG:'$H'|'$X' to unlock]");
        }
        s
    }

    /// Effective wire (heat) state.
    pub fn wire_on(&self) -> bool {
        self.wire && !self.wire_stopped
    }

    pub fn position(&self) -> [f64; 4] {
        self.live_pos()
    }

    pub fn is_alarm(&self) -> bool {
        self.alarm
    }

    pub fn is_idle(&self) -> bool {
        self.cur.is_none() && self.queue.is_empty() && !self.alarm
    }

    pub fn homed(&self) -> bool {
        self.homed
    }

    fn say(&mut self, line: &str) {
        self.out.extend(line.trim_end().as_bytes());
        self.out.extend(b"\r\n");
    }

    fn live_pos(&self) -> [f64; 4] {
        match self.cur {
            Some((Block::Move { to, secs, .. }, t, from)) if secs > 0.0 => {
                let f = (t / secs).clamp(0.0, 1.0);
                std::array::from_fn(|k| from[k] + (to[k] - from[k]) * f)
            }
            _ => self.pos,
        }
    }

    fn moving(&self) -> bool {
        self.cur.is_some() || !self.queue.is_empty()
    }

    fn state_name(&self) -> String {
        if self.alarm {
            "Alarm".into()
        } else if matches!(self.cur, Some((Block::Home { .. }, ..))) {
            "Home".into()
        } else if self.hold && self.moving() {
            "Hold:0".into()
        } else if matches!(self.cur, Some((Block::Move { jog: true, .. }, ..))) {
            "Jog".into()
        } else if self.moving() {
            "Run".into()
        } else {
            "Idle".into()
        }
    }

    fn status(&mut self) {
        let p = self.live_pos();
        let free_planner = self
            .cfg
            .planner
            .saturating_sub(self.queue.len() + usize::from(self.cur.is_some()));
        let free_rx = self.cfg.rx_size.saturating_sub(self.rx.len());
        let s = format!(
            "<{}|MPos:{:.3},{:.3},{:.3},{:.3}|Bf:{},{}|FS:0,{}>",
            self.state_name(),
            p[0],
            p[1],
            p[2],
            p[3],
            free_planner,
            free_rx,
            if self.wire_on() { 1000 } else { 0 }
        );
        self.say(&s);
    }

    /// Advance simulated time by `dt` seconds.
    fn advance(&mut self, mut dt: f64) {
        if self.hold {
            return;
        }
        loop {
            if self.cur.is_none() {
                match self.queue.pop_front() {
                    Some(b) => self.cur = Some((b, 0.0, self.pos)),
                    None => return,
                }
            }
            let (b, t, from) = self.cur.expect("set above");
            let left = b.secs() - t;
            if dt < left {
                self.cur = Some((b, t + dt, from));
                return;
            }
            dt -= left.max(0.0);
            self.cur = None;
            match b {
                Block::Move { to, .. } => self.pos = to,
                Block::Dwell { .. } => {}
                Block::Wire(on) => {
                    self.wire = on;
                    self.log.push(if on {
                        "wire on".into()
                    } else {
                        "wire off".into()
                    });
                }
                Block::Home { .. } => {
                    self.pos = self.cfg.home;
                    self.motion.pos = self.pos;
                    self.homed = true;
                    self.say("[MSG:INFO: Homed]");
                    self.say("ok");
                }
            }
        }
    }

    fn reset(&mut self) {
        let was_moving = self.moving() && !self.hold;
        self.pos = self.live_pos();
        self.rx.clear();
        self.queue.clear();
        self.cur = None;
        self.hold = false;
        self.wire = false;
        self.wire_stopped = false;
        self.motion = Motion::new(self.cfg.axes, self.cfg.rapid);
        self.motion.pos = self.pos;
        self.log.push("reset".into());
        if was_moving {
            self.alarm = true;
            self.homed = false;
            self.say("ALARM:3");
        }
        self.out.extend(BANNER.as_bytes());
        if self.alarm {
            self.say("[MSG:'$H'|'$X' to unlock]");
        }
    }

    fn realtime(&mut self, b: u8) {
        match b {
            rt::STATUS => self.status(),
            rt::FEED_HOLD => {
                if self.moving() {
                    // Stop where we are; the rest of the current block waits.
                    self.hold = true;
                }
            }
            rt::CYCLE_START => {
                self.hold = false;
                self.wire_stopped = false;
            }
            rt::SPINDLE_STOP => {
                if self.hold {
                    self.wire_stopped = !self.wire_stopped;
                    self.log.push(format!("spindle stop {}", self.wire_stopped));
                }
            }
            rt::SOFT_RESET => self.reset(),
            rt::JOG_CANCEL => {
                if matches!(self.cur, Some((Block::Move { jog: true, .. }, ..))) {
                    self.pos = self.live_pos();
                    self.motion.pos = self.pos;
                    self.cur = None;
                    self.queue
                        .retain(|b| !matches!(b, Block::Move { jog: true, .. }));
                }
            }
            _ => {}
        }
    }

    fn within_limits(&self, p: &[f64; 4]) -> bool {
        (0..4).all(|k| {
            let [lo, hi] = self.cfg.limits[k];
            p[k] >= lo - 1e-6 && p[k] <= hi + 1e-6
        })
    }

    fn line(&mut self, line: &str) {
        self.lines += 1;
        if self.fail_on_line == Some(self.lines) {
            self.say("error:20");
            return;
        }
        let up = line.trim().to_ascii_uppercase();
        if let Some(cmd) = up.strip_prefix('$') {
            return self.system(cmd);
        }
        if self.alarm {
            return self.say("error:9");
        }
        let ws = words(&up);
        for &(c, v) in &ws {
            let ok = match c {
                'G' => [0, 1, 4, 17, 21, 54, 90, 91, 93, 94].contains(&(v as u32)),
                'M' => [2, 3, 4, 5, 30].contains(&(v as u32)),
                'F' | 'S' | 'P' | 'N' => true,
                c => self.cfg.axes.contains(&c),
            };
            if !ok {
                return self.say("error:20");
            }
        }
        let has = |l: char, n: u32| ws.iter().any(|&(c, v)| c == l && v as u32 == n);
        let has_axis = ws.iter().any(|(c, _)| self.cfg.axes.contains(c));
        let g93_now = self.motion.inverse_time && !has('G', 94) || has('G', 93);
        if has_axis && g93_now && !ws.iter().any(|(c, _)| *c == 'F') && !has('G', 0) {
            return self.say("error:22");
        }
        let (secs, target) = self.motion.apply(&up);
        if let Some(t) = target {
            if !self.within_limits(&t) {
                self.alarm = true;
                self.queue.clear();
                self.cur = None;
                self.wire = false;
                self.motion.pos = self.pos;
                self.say("ALARM:2");
                return self.say("ok");
            }
            self.queue.push_back(Block::Move {
                to: t,
                secs,
                jog: false,
            });
        } else if has('G', 4) {
            self.queue.push_back(Block::Dwell { secs });
        }
        if has('M', 3) || has('M', 4) {
            self.queue.push_back(Block::Wire(true));
        }
        if has('M', 5) || has('M', 2) || has('M', 30) {
            self.queue.push_back(Block::Wire(false));
        }
        self.say("ok");
    }

    fn system(&mut self, cmd: &str) {
        match cmd {
            "H" => {
                if self.moving() {
                    return self.say("error:8");
                }
                self.alarm = false;
                self.queue.push_back(Block::Home {
                    secs: self.cfg.homing_secs,
                });
                // "ok" comes when homing finishes.
            }
            "X" => {
                self.alarm = false;
                self.say("[MSG:Caution: Unlocked]");
                self.say("ok");
            }
            "I" => {
                self.say("[VER:3.9 FluidNC v4.1.1 (wingcut simulator):]");
                self.say("ok");
            }
            c if c.starts_with("J=") => {
                if self.alarm {
                    return self.say("error:9");
                }
                let mut m = self.motion.clone();
                m.motion = 1;
                m.inverse_time = false;
                let body = &c[2..];
                if !body.contains('F') {
                    return self.say("error:16");
                }
                let (secs, target) = m.apply(body);
                match target {
                    Some(t) if self.within_limits(&t) => {
                        self.queue.push_back(Block::Move {
                            to: t,
                            secs,
                            jog: true,
                        });
                        self.motion.pos = t;
                        self.say("ok");
                    }
                    Some(_) => self.say("error:15"),
                    None => self.say("error:16"),
                }
            }
            _ => self.say("ok"),
        }
    }

    /// Parse whole lines from RX while the planner has room.
    fn process(&mut self) {
        loop {
            let homing = matches!(self.cur, Some((Block::Home { .. }, ..)))
                || self.queue.iter().any(|b| matches!(b, Block::Home { .. }));
            let full = self.queue.len() + usize::from(self.cur.is_some()) >= self.cfg.planner;
            if homing || full {
                return;
            }
            let Some(nl) = self.rx.iter().position(|&b| b == b'\n') else {
                return;
            };
            let line: Vec<u8> = self.rx.drain(..=nl).collect();
            let text = String::from_utf8_lossy(&line).trim().to_string();
            if !text.is_empty() {
                self.line(&text);
            }
        }
    }

    fn tick(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f64() * self.cfg.speed;
        self.last = now;
        self.clock += dt;
        self.advance(dt);
        self.process();
    }

    fn receive(&mut self, data: &[u8]) {
        for &b in data {
            match b {
                rt::STATUS
                | rt::FEED_HOLD
                | rt::CYCLE_START
                | rt::SOFT_RESET
                | rt::JOG_CANCEL
                | rt::SPINDLE_STOP => {
                    self.tick();
                    self.realtime(b)
                }
                b'\r' => {}
                b => self.rx.push(b),
            }
        }
        if self.rx.len() > self.cfg.rx_size {
            self.log
                .push(format!("RX overflow ({} bytes)", self.rx.len()));
        }
        self.max_rx = self.max_rx.max(self.rx.len());
        self.tick();
    }
}

/// A [`Link`] to a simulator. Clone the `Arc` to inspect it from tests.
pub struct SimLink(pub Arc<Mutex<Sim>>);

impl SimLink {
    pub fn new(cfg: SimConfig) -> (SimLink, Arc<Mutex<Sim>>) {
        let sim = Arc::new(Mutex::new(Sim::new(cfg)));
        (SimLink(sim.clone()), sim)
    }
}

fn gone() -> io::Error {
    io::Error::new(io::ErrorKind::ConnectionAborted, "simulated disconnect")
}

impl Link for SimLink {
    fn send(&mut self, data: &[u8]) -> io::Result<()> {
        let mut s = self.0.lock().expect("sim lock");
        if s.disconnect_at.is_some_and(|t| s.clock >= t) {
            return Err(gone());
        }
        s.receive(data);
        Ok(())
    }

    fn recv(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<usize> {
        for attempt in 0..2 {
            {
                let mut s = self.0.lock().expect("sim lock");
                s.tick();
                if s.disconnect_at.is_some_and(|t| s.clock >= t) {
                    return Err(gone());
                }
                if !s.out.is_empty() {
                    let n = s.out.len().min(buf.len());
                    for b in buf.iter_mut().take(n) {
                        *b = s.out.pop_front().expect("len checked");
                    }
                    return Ok(n);
                }
            }
            if attempt == 0 {
                std::thread::sleep(timeout.min(Duration::from_millis(5)));
            }
        }
        Ok(0)
    }

    fn describe(&self) -> String {
        "simulator".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn talk(l: &mut SimLink, s: &str) -> String {
        l.send(s.as_bytes()).unwrap();
        let mut out = String::new();
        let mut buf = [0u8; 256];
        for _ in 0..20 {
            let n = l.recv(&mut buf, Duration::from_millis(5)).unwrap();
            out.push_str(&String::from_utf8_lossy(&buf[..n]));
        }
        out
    }

    fn fast() -> SimConfig {
        SimConfig {
            speed: 1000.0,
            ..SimConfig::default()
        }
    }

    #[test]
    fn answers_ok_status_and_errors() {
        let (mut l, _) = SimLink::new(fast());
        assert!(talk(&mut l, "").contains("FluidNC"));
        assert!(talk(&mut l, "G21 G90\n").contains("ok"));
        assert!(talk(&mut l, "?").contains("<Idle|MPos:0.000,0.000,0.000,0.000"));
        assert!(talk(&mut l, "G7\n").contains("error:20"));
        assert!(
            talk(&mut l, "G93\nG1 X1\n").contains("error:22"),
            "G93 needs F on every move"
        );
    }

    #[test]
    fn executes_moves_with_timing_and_wire() {
        let (mut l, sim) = SimLink::new(SimConfig {
            speed: 100.0,
            ..SimConfig::default()
        });
        talk(&mut l, "M3 S1000\nG1 X10 Y10 Z10 A10 F600\n");
        // 20 mm at 600 mm/min = 2 s = 20 ms at 100×.
        std::thread::sleep(Duration::from_millis(60));
        let s = talk(&mut l, "?");
        assert!(s.contains("<Idle|MPos:10.000,10.000,10.000,10.000"), "{s}");
        assert!(sim.lock().unwrap().wire_on());
        talk(&mut l, "M5\n");
        assert!(!sim.lock().unwrap().wire_on());
    }

    #[test]
    fn hold_freezes_motion_and_spindle_stop_cuts_the_wire() {
        let (mut l, sim) = SimLink::new(SimConfig {
            speed: 10.0,
            ..SimConfig::default()
        });
        talk(&mut l, "M3\nG1 X100 F600\n"); // 10 s
        talk(&mut l, "!");
        let p1 = sim.lock().unwrap().position()[0];
        std::thread::sleep(Duration::from_millis(30));
        assert!(talk(&mut l, "?").contains("Hold:0"));
        assert_eq!(sim.lock().unwrap().position()[0], p1);
        assert!(sim.lock().unwrap().wire_on());
        l.send(&[rt::SPINDLE_STOP]).unwrap();
        assert!(!sim.lock().unwrap().wire_on(), "wire off while held");
        talk(&mut l, "~");
        assert!(sim.lock().unwrap().wire_on(), "restored on resume");
    }

    #[test]
    fn reset_during_motion_alarms_but_after_hold_does_not() {
        let (mut l, sim) = SimLink::new(SimConfig {
            speed: 1.0,
            ..SimConfig::default()
        });
        talk(&mut l, "M3\nG1 X100 F600\n");
        let s = talk(&mut l, "\u{18}");
        assert!(s.contains("ALARM:3") && s.contains("Grbl"), "{s}");
        assert!(!sim.lock().unwrap().wire_on());
        talk(&mut l, "$X\n");
        talk(&mut l, "G1 X100 F600\n!");
        let s = talk(&mut l, "\u{18}");
        assert!(!s.contains("ALARM"), "{s}");
    }

    #[test]
    fn soft_limits_alarm_and_lock_out() {
        let (mut l, _) = SimLink::new(fast());
        let s = talk(&mut l, "G1 X2000 F600\n");
        assert!(s.contains("ALARM:2"), "{s}");
        assert!(talk(&mut l, "G1 X1 F600\n").contains("error:9"));
        assert!(talk(&mut l, "$J=G91 X5000 F500\n").contains("error:9"));
        talk(&mut l, "$X\n");
        assert!(talk(&mut l, "$J=G91 X5000 F500\n").contains("error:15"));
    }

    #[test]
    fn homing_and_jogging() {
        let (mut l, sim) = SimLink::new(SimConfig {
            require_homing: true,
            home: [5.0, 290.0, 5.0, 290.0],
            ..fast()
        });
        assert!(talk(&mut l, "G1 X1 F100\n").contains("error:9"));
        let s = talk(&mut l, "$H\n");
        assert!(s.contains("Homed") && s.contains("ok"), "{s}");
        assert!(sim.lock().unwrap().homed());
        talk(&mut l, "$J=G91 X10 Z10 F1000\n");
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(sim.lock().unwrap().position(), [15.0, 290.0, 15.0, 290.0]);
    }

    #[test]
    fn planner_back_pressure_holds_lines_in_rx() {
        let (mut l, sim) = SimLink::new(SimConfig {
            speed: 1.0,
            ..SimConfig::default()
        });
        let mut prog = String::new();
        for i in 0..20 {
            prog += &format!("G1 X{} F60\n", i + 1); // 1 s each
        }
        let s = talk(&mut l, &prog);
        let oks = s.matches("ok").count();
        assert_eq!(oks, 16, "only a planner's worth is accepted");
        assert!(
            sim.lock()
                .unwrap()
                .log
                .iter()
                .any(|e| e.contains("overflow")),
            "sending 20 lines at once overflows RX"
        );
    }
}
