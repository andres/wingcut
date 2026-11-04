//! Riden RD60xx bench supplies (RD6006, RD6006P, RD6012, RD6018, RD6024)
//! over Modbus RTU: set the wire current, switch the output, read live
//! volts and amps. Plus a simulated supply for tests.
//!
//! Register map (holding registers, as used by the open-source `rd6006`
//! and RidenHack tools): 0 model, 8 V set, 9 I set, 10 V out, 11 I out,
//! 17 CV/CC, 18 output on. Scales depend on the model.

use crate::link::Link;
use std::io;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const REG_MODEL: u16 = 0;
const REG_V_SET: u16 = 8;
const REG_I_SET: u16 = 9;
const REG_V_OUT: u16 = 10;
const REG_I_OUT: u16 = 11;
const REG_OUTPUT: u16 = 18;

/// Modbus CRC-16 (poly 0xA001, init 0xFFFF), sent low byte first.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= b as u16;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xA001
            } else {
                crc >> 1
            };
        }
    }
    crc
}

fn with_crc(mut f: Vec<u8>) -> Vec<u8> {
    let c = crc16(&f);
    f.extend([(c & 0xFF) as u8, (c >> 8) as u8]);
    f
}

fn check_crc(f: &[u8]) -> bool {
    f.len() >= 4 && {
        let (body, crc) = f.split_at(f.len() - 2);
        crc16(body) == u16::from_le_bytes([crc[0], crc[1]])
    }
}

/// Scales for a model: (volts per count, amps per count).
pub fn scales(model: u16) -> (f64, f64) {
    match model {
        60065 => (0.001, 0.0001),      // RD6006P
        60121..=60129 => (0.01, 0.01), // RD6012
        60181..=60189 => (0.01, 0.01), // RD6018
        60241..=60249 => (0.01, 0.01), // RD6024
        _ => (0.01, 0.001),            // RD6006 (60062) and default
    }
}

pub fn model_name(model: u16) -> String {
    match model {
        60062 => "RD6006".into(),
        60065 => "RD6006P".into(),
        60121..=60129 => "RD6012".into(),
        60181..=60189 => "RD6018".into(),
        60241..=60249 => "RD6024".into(),
        m => format!("Riden model {m}"),
    }
}

/// A Modbus RTU client for one supply on a link.
pub struct Riden {
    link: Box<dyn Link>,
    addr: u8,
    pub model: u16,
    v_scale: f64,
    i_scale: f64,
}

fn proto(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

impl Riden {
    /// Open and identify the supply.
    pub fn open(link: Box<dyn Link>, addr: u8) -> io::Result<Riden> {
        let mut r = Riden {
            link,
            addr,
            model: 0,
            v_scale: 0.01,
            i_scale: 0.001,
        };
        let model = r.read(REG_MODEL, 1)?[0];
        let (v, i) = scales(model);
        (r.model, r.v_scale, r.i_scale) = (model, v, i);
        Ok(r)
    }

    fn transact(&mut self, req: Vec<u8>, reply_len: usize) -> io::Result<Vec<u8>> {
        self.link.send(&with_crc(req))?;
        let mut got = Vec::new();
        let deadline = Instant::now() + Duration::from_millis(500);
        let mut buf = [0u8; 64];
        while got.len() < reply_len {
            if Instant::now() > deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "no reply from the supply (check port, baud and address)",
                ));
            }
            let n = self.link.recv(&mut buf, Duration::from_millis(20))?;
            got.extend_from_slice(&buf[..n]);
            // Exception reply: addr, func|0x80, code, crc.
            if got.len() >= 5 && got[1] & 0x80 != 0 {
                return Err(proto(format!(
                    "supply rejected the request (Modbus exception {})",
                    got[2]
                )));
            }
        }
        if !check_crc(&got[..reply_len]) {
            return Err(proto("bad CRC from the supply"));
        }
        Ok(got[..reply_len].to_vec())
    }

    pub fn read(&mut self, start: u16, count: u16) -> io::Result<Vec<u16>> {
        let req = vec![
            self.addr,
            0x03,
            (start >> 8) as u8,
            start as u8,
            (count >> 8) as u8,
            count as u8,
        ];
        let r = self.transact(req, 5 + 2 * count as usize)?;
        Ok(r[3..3 + 2 * count as usize]
            .chunks(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect())
    }

    pub fn write(&mut self, reg: u16, value: u16) -> io::Result<()> {
        let req = vec![
            self.addr,
            0x06,
            (reg >> 8) as u8,
            reg as u8,
            (value >> 8) as u8,
            value as u8,
        ];
        self.transact(req, 8).map(|_| ())
    }

    pub fn set_current(&mut self, amps: f64) -> io::Result<()> {
        self.write(
            REG_I_SET,
            (amps / self.i_scale).round().clamp(0.0, 65535.0) as u16,
        )
    }

    pub fn set_voltage(&mut self, volts: f64) -> io::Result<()> {
        self.write(
            REG_V_SET,
            (volts / self.v_scale).round().clamp(0.0, 65535.0) as u16,
        )
    }

    pub fn set_output(&mut self, on: bool) -> io::Result<()> {
        self.write(REG_OUTPUT, u16::from(on))
    }

    /// Set voltage/current and live output in one read (registers 8..=18).
    pub fn reading(&mut self) -> io::Result<Reading> {
        let r = self.read(REG_V_SET, 11)?;
        let at = |reg: u16| r[(reg - REG_V_SET) as usize] as f64;
        Ok(Reading {
            v_set: at(REG_V_SET) * self.v_scale,
            i_set: at(REG_I_SET) * self.i_scale,
            v_out: at(REG_V_OUT) * self.v_scale,
            i_out: at(REG_I_OUT) * self.i_scale,
            output: at(REG_OUTPUT) != 0.0,
        })
    }
}

/// One measurement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reading {
    pub v_set: f64,
    pub i_set: f64,
    pub v_out: f64,
    pub i_out: f64,
    pub output: bool,
}

impl Reading {
    /// Wire resistance (Ω) when current flows.
    pub fn resistance(&self) -> Option<f64> {
        (self.i_out > 0.05).then(|| self.v_out / self.i_out)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RidenCommand {
    SetCurrent(f64),
    SetVoltage(f64),
    Output(bool),
    Disconnect,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RidenEvent {
    Connected { model: String },
    Reading(Reading),
    Error(String),
    Disconnected(String),
}

/// Background poller for a supply (about 3 readings a second).
pub struct RidenHandle {
    tx: Sender<RidenCommand>,
    rx: Receiver<RidenEvent>,
    thread: Option<JoinHandle<()>>,
}

impl RidenHandle {
    pub fn start(
        link: Box<dyn Link>,
        addr: u8,
        notify: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> RidenHandle {
        let (tx, cmd_rx) = mpsc::channel();
        let (ev_tx, rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("wingcut-riden".into())
            .spawn(move || {
                let emit = |e: RidenEvent| {
                    let _ = ev_tx.send(e);
                    if let Some(n) = &notify {
                        n();
                    }
                };
                let mut r = match Riden::open(link, addr) {
                    Ok(r) => r,
                    Err(e) => return emit(RidenEvent::Disconnected(format!("{e}"))),
                };
                emit(RidenEvent::Connected {
                    model: model_name(r.model),
                });
                let res = (|| -> io::Result<()> {
                    loop {
                        loop {
                            match cmd_rx.try_recv() {
                                Ok(RidenCommand::SetCurrent(a)) => r.set_current(a)?,
                                Ok(RidenCommand::SetVoltage(v)) => r.set_voltage(v)?,
                                Ok(RidenCommand::Output(on)) => r.set_output(on)?,
                                Ok(RidenCommand::Disconnect) | Err(TryRecvError::Disconnected) => {
                                    // Leave the supply safe: output off.
                                    let _ = r.set_output(false);
                                    return Ok(());
                                }
                                Err(TryRecvError::Empty) => break,
                            }
                        }
                        emit(RidenEvent::Reading(r.reading()?));
                        std::thread::sleep(Duration::from_millis(300));
                    }
                })();
                match res {
                    Ok(()) => emit(RidenEvent::Disconnected("disconnected".into())),
                    Err(e) => emit(RidenEvent::Disconnected(e.to_string())),
                }
            })
            .expect("spawn riden thread");
        RidenHandle {
            tx,
            rx,
            thread: Some(thread),
        }
    }

    pub fn send(&self, c: RidenCommand) {
        let _ = self.tx.send(c);
    }

    pub fn poll(&self) -> Vec<RidenEvent> {
        self.rx.try_iter().collect()
    }

    pub fn next(&self, timeout: Duration) -> Option<RidenEvent> {
        self.rx.recv_timeout(timeout).ok()
    }
}

impl Drop for RidenHandle {
    fn drop(&mut self) {
        let _ = self.tx.send(RidenCommand::Disconnect);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Watches readings during a cut for a broken wire or a lost contact: the
/// board says the wire is on, the supply's output is on, yet (almost) no
/// current flows.
#[derive(Debug, Clone)]
pub struct BreakDetector {
    /// Trip when current < `fraction` × set current...
    pub fraction: f64,
    /// ...for this many readings in a row.
    pub readings: usize,
    low: usize,
}

impl BreakDetector {
    pub fn new(fraction: f64) -> BreakDetector {
        BreakDetector {
            fraction,
            readings: 3,
            low: 0,
        }
    }

    /// Feed a reading; `wire_should_be_on` comes from the board's status.
    /// Returns true once when the wire looks broken.
    pub fn update(&mut self, r: &Reading, wire_should_be_on: bool) -> bool {
        if wire_should_be_on && r.output && r.i_set > 0.0 && r.i_out < self.fraction * r.i_set {
            self.low += 1;
            if self.low == self.readings {
                return true;
            }
        } else {
            self.low = 0;
        }
        false
    }
}

/// Simulated supply state: a constant-current source into the wire.
pub struct FakeRiden {
    pub model: u16,
    pub addr: u8,
    regs: [u16; 20],
    /// Wire resistance (Ω); `None` = broken wire (open circuit).
    pub wire_ohms: Option<f64>,
    /// Is the controller's MOSFET closing the circuit? (wire on)
    pub load: Box<dyn Fn() -> bool + Send>,
    out: Vec<u8>,
    rx: Vec<u8>,
    pub writes: Vec<(u16, u16)>,
}

impl FakeRiden {
    pub fn new(load: Box<dyn Fn() -> bool + Send>) -> FakeRiden {
        let mut regs = [0u16; 20];
        regs[REG_MODEL as usize] = 60062;
        regs[REG_V_SET as usize] = 4500; // 45.00 V
        regs[REG_I_SET as usize] = 1000; // 1.000 A
        FakeRiden {
            model: 60062,
            addr: 1,
            regs,
            wire_ohms: Some(15.0),
            load,
            out: Vec::new(),
            rx: Vec::new(),
            writes: Vec::new(),
        }
    }

    fn measure(&mut self) {
        let (vs, is) = scales(self.model);
        let v_set = self.regs[REG_V_SET as usize] as f64 * vs;
        let i_set = self.regs[REG_I_SET as usize] as f64 * is;
        let on = self.regs[REG_OUTPUT as usize] != 0 && (self.load)();
        let (v, i) = match (on, self.wire_ohms) {
            (false, _) => (0.0, 0.0),
            (true, None) => (v_set, 0.0),
            // Constant current until the voltage limit is reached.
            (true, Some(r)) => {
                let v = (i_set * r).min(v_set);
                (v, v / r)
            }
        };
        self.regs[REG_V_OUT as usize] = (v / vs).round() as u16;
        self.regs[REG_I_OUT as usize] = (i / is).round() as u16;
    }

    fn handle(&mut self, f: &[u8]) {
        if f[0] != self.addr || !check_crc(f) {
            return; // ignore, like a real bus device
        }
        let reg = u16::from_be_bytes([f[2], f[3]]);
        let val = u16::from_be_bytes([f[4], f[5]]);
        self.measure();
        match f[1] {
            0x03 => {
                if reg as usize + val as usize > self.regs.len() {
                    self.out.extend(with_crc(vec![self.addr, 0x83, 2]));
                    return;
                }
                let mut r = vec![self.addr, 0x03, (val * 2) as u8];
                for k in reg..reg + val {
                    r.extend(self.regs[k as usize].to_be_bytes());
                }
                self.out.extend(with_crc(r));
            }
            0x06 => {
                if reg as usize >= self.regs.len() {
                    self.out.extend(with_crc(vec![self.addr, 0x86, 2]));
                    return;
                }
                self.regs[reg as usize] = val;
                self.writes.push((reg, val));
                self.out.extend_from_slice(f);
            }
            func => self.out.extend(with_crc(vec![self.addr, func | 0x80, 1])),
        }
    }

    pub fn output_on(&self) -> bool {
        self.regs[REG_OUTPUT as usize] != 0
    }

    pub fn current_set(&self) -> f64 {
        self.regs[REG_I_SET as usize] as f64 * scales(self.model).1
    }
}

/// A [`Link`] to a [`FakeRiden`].
pub struct FakeRidenLink(pub Arc<Mutex<FakeRiden>>);

impl Link for FakeRidenLink {
    fn send(&mut self, data: &[u8]) -> io::Result<()> {
        let mut f = self.0.lock().expect("fake riden lock");
        f.rx.extend_from_slice(data);
        if f.rx.len() >= 8 {
            let frame: Vec<u8> = f.rx.drain(..8).collect();
            f.handle(&frame);
        }
        Ok(())
    }

    fn recv(&mut self, buf: &mut [u8], _timeout: Duration) -> io::Result<usize> {
        let mut f = self.0.lock().expect("fake riden lock");
        let n = f.out.len().min(buf.len());
        buf[..n].copy_from_slice(&f.out[..n]);
        f.out.drain(..n);
        Ok(n)
    }

    fn describe(&self) -> String {
        "simulated Riden".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn crc_matches_the_modbus_reference() {
        // "01 03 00 00 00 01" → CRC 0x0A84, sent as 84 0A.
        assert_eq!(crc16(&[0x01, 0x03, 0x00, 0x00, 0x00, 0x01]), 0x0A84);
        assert_eq!(with_crc(vec![1, 3, 0, 0, 0, 1])[6..], [0x84, 0x0A]);
    }

    fn fake(load: bool) -> (Riden, Arc<Mutex<FakeRiden>>, Arc<AtomicBool>) {
        let on = Arc::new(AtomicBool::new(load));
        let o = on.clone();
        let f = Arc::new(Mutex::new(FakeRiden::new(Box::new(move || {
            o.load(Ordering::SeqCst)
        }))));
        let r = Riden::open(Box::new(FakeRidenLink(f.clone())), 1).unwrap();
        (r, f, on)
    }

    #[test]
    fn identifies_sets_and_reads() {
        let (mut r, f, _) = fake(true);
        assert_eq!(model_name(r.model), "RD6006");
        r.set_voltage(45.0).unwrap();
        r.set_current(1.9).unwrap();
        r.set_output(true).unwrap();
        assert!((f.lock().unwrap().current_set() - 1.9).abs() < 1e-9);
        let m = r.reading().unwrap();
        assert!((m.i_set - 1.9).abs() < 1e-9 && m.output);
        assert!(
            (m.i_out - 1.9).abs() < 0.002 && (m.v_out - 28.5).abs() < 0.01,
            "{m:?}"
        );
        assert!((m.resistance().unwrap() - 15.0).abs() < 0.05);
    }

    #[test]
    fn voltage_limit_caps_the_current_and_a_broken_wire_reads_zero() {
        let (mut r, f, _) = fake(true);
        r.set_voltage(20.0).unwrap();
        r.set_current(2.0).unwrap(); // 2 A × 15 Ω = 30 V > 20 V limit
        r.set_output(true).unwrap();
        let m = r.reading().unwrap();
        assert!((m.v_out - 20.0).abs() < 0.01 && (m.i_out - 20.0 / 15.0).abs() < 0.002);
        f.lock().unwrap().wire_ohms = None;
        let m = r.reading().unwrap();
        assert_eq!(m.i_out, 0.0);
    }

    #[test]
    fn break_detector_needs_the_wire_on_and_several_low_readings() {
        let mut d = BreakDetector::new(0.5);
        let ok = Reading {
            v_set: 45.0,
            i_set: 1.9,
            v_out: 28.5,
            i_out: 1.9,
            output: true,
        };
        let broken = Reading {
            i_out: 0.0,
            v_out: 45.0,
            ..ok
        };
        assert!(
            !d.update(&broken, false),
            "wire off by the board: no current expected"
        );
        assert!(!d.update(&ok, true));
        assert!(!d.update(&broken, true));
        assert!(!d.update(&broken, true));
        assert!(d.update(&broken, true), "third low reading trips");
        assert!(!d.update(&broken, true), "trips once");
        assert!(!d.update(&ok, true));
    }

    #[test]
    fn background_handle_reports_and_switches_off_on_disconnect() {
        let on = Arc::new(AtomicBool::new(true));
        let o = on.clone();
        let f = Arc::new(Mutex::new(FakeRiden::new(Box::new(move || {
            o.load(Ordering::SeqCst)
        }))));
        let h = RidenHandle::start(Box::new(FakeRidenLink(f.clone())), 1, None);
        assert!(matches!(
            h.next(Duration::from_secs(2)),
            Some(RidenEvent::Connected { .. })
        ));
        h.send(RidenCommand::SetCurrent(1.5));
        h.send(RidenCommand::Output(true));
        let t0 = Instant::now();
        loop {
            if let Some(RidenEvent::Reading(r)) = h.next(Duration::from_millis(500))
                && r.output
                && (r.i_out - 1.5).abs() < 0.01
            {
                break;
            }
            assert!(t0.elapsed() < Duration::from_secs(3));
        }
        drop(h);
        assert!(
            !f.lock().unwrap().output_on(),
            "output switched off when we let go"
        );
    }

    #[test]
    fn wrong_address_times_out_clearly() {
        let f = Arc::new(Mutex::new(FakeRiden::new(Box::new(|| false))));
        let e = Riden::open(Box::new(FakeRidenLink(f)), 7).err().unwrap();
        assert!(e.to_string().contains("no reply"), "{e}");
    }
}
