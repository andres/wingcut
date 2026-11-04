//! The controller against the simulated board, with real wingcut G-code.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wingcut::config::{Machine, Wing};
use wingcut::cut::plan_panel;
use wingcut::wing::WingModel;
use wingcut_machine::controller::{Command, Controller, Event, Job, Options};
use wingcut_machine::grbl::State;
use wingcut_machine::sim::{Sim, SimConfig, SimLink};

const AXES: [char; 4] = ['X', 'Y', 'Z', 'A'];

/// G-code for an example panel, plus the machine.
fn example(panel: &str) -> (String, Machine) {
    let ex = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples");
    let m = Machine::load(&ex.join("machine.toml")).unwrap();
    let w = Wing::load(&ex.join("wing.toml")).unwrap();
    let model = WingModel::new(w, m.max_segment).unwrap();
    let plan = plan_panel(&model, &m, model.wing.panel(panel).unwrap()).unwrap();
    (wingcut::gcode::emit(&plan, &m, "test"), m)
}

fn start(speed: f64) -> (Controller, Arc<Mutex<Sim>>) {
    let (link, sim) = SimLink::new(SimConfig {
        speed,
        ..SimConfig::default()
    });
    (
        Controller::start(Box::new(link), Options::default(), None),
        sim,
    )
}

/// Collect events until `pred` matches one (or panic on timeout).
fn until(c: &Controller, secs: f64, mut pred: impl FnMut(&Event) -> bool) -> Vec<Event> {
    let deadline = Instant::now() + Duration::from_secs_f64(secs);
    let mut seen = Vec::new();
    while Instant::now() < deadline {
        if let Some(e) = c.next(Duration::from_millis(50)) {
            let hit = pred(&e);
            seen.push(e);
            if hit {
                return seen;
            }
        }
    }
    let tail: Vec<_> = seen.iter().rev().take(5).collect();
    panic!("timed out; last events: {tail:?}");
}

fn state_is(c: &Controller, want: State) {
    until(
        c,
        5.0,
        |e| matches!(e, Event::Status { status, .. } if status.state == want),
    );
}

fn wait_for(secs: f64, what: &str, mut f: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(
            t0.elapsed().as_secs_f64() < secs,
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn streams_a_whole_panel_without_overflowing() {
    let (gcode, m) = example("center-L");
    let (c, sim) = start(3000.0);
    state_is(&c, State::Idle);
    let job = Job::from_gcode("center-L", &gcode, AXES, m.feed_rapid, false);
    assert!(job.total_secs > 150.0, "{}", job.total_secs);
    let n = job.lines.len();
    c.send(Command::Start(job));
    let ev = until(&c, 30.0, |e| {
        matches!(e, Event::JobFinished { .. } | Event::JobFailed { .. })
    });
    assert!(
        matches!(ev.last(), Some(Event::JobFinished { .. })),
        "{:?}",
        ev.last()
    );
    assert!(ev.iter().any(
        |e| matches!(e, Event::Progress { acked, total, .. } if acked == total && *total == n)
    ));
    let s = sim.lock().unwrap();
    assert!(!s.log.iter().any(|l| l.contains("overflow")), "{:?}", s.log);
    assert!(s.max_rx <= 127, "RX never exceeds the buffer: {}", s.max_rx);
    assert!(
        s.log.contains(&"wire on".to_string()) && !s.wire_on(),
        "wire off at the end"
    );
    assert_eq!(s.position()[0], m.park_x, "parked");
}

#[test]
fn pause_cuts_the_wire_and_resume_restores_it() {
    let (gcode, m) = example("tip-L");
    let (c, sim) = start(100.0);
    state_is(&c, State::Idle);
    c.send(Command::Start(Job::from_gcode(
        "tip-L",
        &gcode,
        AXES,
        m.feed_rapid,
        false,
    )));
    wait_for(5.0, "wire on", || sim.lock().unwrap().wire_on());
    std::thread::sleep(Duration::from_millis(100));
    c.send(Command::Pause);
    until(&c, 5.0, |e| matches!(e, Event::Paused));
    wait_for(3.0, "wire off while paused", || {
        !sim.lock().unwrap().wire_on()
    });
    let p = sim.lock().unwrap().position();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(sim.lock().unwrap().position(), p, "no motion while paused");
    c.send(Command::Resume);
    until(&c, 5.0, |e| matches!(e, Event::Resumed));
    wait_for(3.0, "wire back on", || sim.lock().unwrap().wire_on());
    std::thread::sleep(Duration::from_millis(50));
    assert_ne!(sim.lock().unwrap().position(), p, "moving again");
    c.send(Command::Stop);
    until(&c, 8.0, |e| matches!(e, Event::JobStopped { .. }));
}

#[test]
fn stop_holds_then_resets_without_alarm_and_kills_the_wire() {
    let (gcode, m) = example("center-L");
    let (c, sim) = start(100.0);
    state_is(&c, State::Idle);
    c.send(Command::Start(Job::from_gcode(
        "center-L",
        &gcode,
        AXES,
        m.feed_rapid,
        false,
    )));
    wait_for(5.0, "wire on", || sim.lock().unwrap().wire_on());
    std::thread::sleep(Duration::from_millis(100));
    c.send(Command::Stop);
    let ev = until(&c, 8.0, |e| matches!(e, Event::JobStopped { .. }));
    assert!(
        !ev.iter().any(|e| matches!(e, Event::Alarm(_))),
        "hold before reset: no alarm"
    );
    let s = sim.lock().unwrap();
    assert!(!s.wire_on() && !s.is_alarm() && s.is_idle(), "{:?}", s.log);
    assert!(s.log.contains(&"reset".to_string()));
}

#[test]
fn an_error_line_fails_the_job_with_its_line_number() {
    let (gcode, m) = example("center-L");
    let (c, sim) = start(1000.0);
    sim.lock().unwrap().fail_on_line = Some(20);
    state_is(&c, State::Idle);
    let job = Job::from_gcode("center-L", &gcode, AXES, m.feed_rapid, false);
    let src_line = job.lines[19].n;
    c.send(Command::Start(job));
    let ev = until(&c, 8.0, |e| matches!(e, Event::JobFailed { .. }));
    let Some(Event::JobFailed { reason, .. }) = ev.last() else {
        unreachable!()
    };
    assert!(
        reason.contains(&format!("line {src_line}")) && reason.contains("error:20"),
        "{reason}"
    );
    assert!(!sim.lock().unwrap().wire_on());
}

#[test]
fn an_alarm_fails_the_job_and_starting_in_alarm_is_refused() {
    let (c, sim) = start(1000.0);
    state_is(&c, State::Idle);
    // Program outside the travel: the simulator raises a soft-limit alarm.
    let gcode = "G21 G90 G94\nM3 S1000\nG1 X100 Y10 Z100 A10 F800\nG1 X5000 F800\nM5\n";
    c.send(Command::Start(Job::from_gcode(
        "bad", gcode, AXES, 800.0, false,
    )));
    let ev = until(&c, 8.0, |e| matches!(e, Event::JobFailed { .. }));
    assert!(
        ev.iter()
            .any(|e| matches!(e, Event::Alarm(a) if a.contains("ALARM:2"))),
        "{ev:?}"
    );
    assert!(!sim.lock().unwrap().wire_on());
}

#[test]
fn a_job_is_refused_in_alarm_until_unlocked() {
    let (link, _sim) = SimLink::new(SimConfig {
        speed: 1000.0,
        require_homing: true,
        ..SimConfig::default()
    });
    let c = Controller::start(Box::new(link), Options::default(), None);
    state_is(&c, State::Alarm);
    c.send(Command::Start(Job::from_gcode(
        "x",
        "G1 X1 F100\n",
        AXES,
        800.0,
        false,
    )));
    until(
        &c,
        3.0,
        |e| matches!(e, Event::Error(m) if m.contains("alarm")),
    );
    c.send(Command::Unlock);
    state_is(&c, State::Idle);
}

#[test]
fn dry_run_never_heats_the_wire() {
    let (gcode, m) = example("tip-R");
    let (c, sim) = start(3000.0);
    state_is(&c, State::Idle);
    let job = Job::from_gcode("tip-R", &gcode, AXES, m.feed_rapid, true);
    assert!(!job.lines.iter().any(|l| l.text.starts_with("M3")));
    c.send(Command::Start(job));
    until(&c, 30.0, |e| matches!(e, Event::JobFinished { .. }));
    assert!(!sim.lock().unwrap().log.contains(&"wire on".to_string()));
}

#[test]
fn losing_the_link_mid_job_is_reported() {
    let (gcode, m) = example("center-L");
    let (c, sim) = start(1000.0);
    state_is(&c, State::Idle);
    c.send(Command::Start(Job::from_gcode(
        "center-L",
        &gcode,
        AXES,
        m.feed_rapid,
        false,
    )));
    until(&c, 5.0, |e| matches!(e, Event::JobStarted { .. }));
    // Drop the link 20 simulated seconds into the job (relative, so slow
    // CI machines don't drop it before the job starts).
    {
        let mut s = sim.lock().unwrap();
        s.disconnect_at = Some(s.clock() + 20.0);
    }
    let ev = until(&c, 10.0, |e| matches!(e, Event::Disconnected(_)));
    assert!(
        ev.iter()
            .any(|e| matches!(e, Event::JobFailed { reason, .. } if reason.contains("E-stop"))),
        "{ev:?}"
    );
}

#[test]
fn home_jog_and_manual_commands() {
    let (link, sim) = SimLink::new(SimConfig {
        speed: 1000.0,
        require_homing: true,
        home: [0.0, 280.0, 0.0, 280.0],
        ..SimConfig::default()
    });
    let c = Controller::start(Box::new(link), Options::default(), None);
    state_is(&c, State::Alarm);
    c.send(Command::Home);
    until(
        &c,
        5.0,
        |e| matches!(e, Event::Console(m) if m.contains("Homed")),
    );
    state_is(&c, State::Idle);
    c.send(Command::Jog {
        moves: vec![('X', 50.0), ('Z', 50.0)],
        feed: 1000.0,
    });
    until(
        &c,
        5.0,
        |e| matches!(e, Event::Status { mpos: Some(p), .. } if p[0] == 50.0 && p[2] == 50.0),
    );
    c.send(Command::Send("G7".into()));
    until(
        &c,
        3.0,
        |e| matches!(e, Event::Error(m) if m.contains("error:20")),
    );
    assert!(sim.lock().unwrap().homed());
}

#[test]
fn manual_commands_are_refused_during_a_job() {
    let (gcode, m) = example("center-L");
    let (c, _sim) = start(50.0);
    state_is(&c, State::Idle);
    c.send(Command::Start(Job::from_gcode(
        "center-L",
        &gcode,
        AXES,
        m.feed_rapid,
        false,
    )));
    until(&c, 3.0, |e| matches!(e, Event::JobStarted { .. }));
    c.send(Command::Jog {
        moves: vec![('X', 10.0)],
        feed: 500.0,
    });
    until(
        &c,
        3.0,
        |e| matches!(e, Event::Error(m) if m.contains("job is running")),
    );
    c.send(Command::Stop);
    until(&c, 8.0, |e| matches!(e, Event::JobStopped { .. }));
}
