//! The grbl / FluidNC text protocol: replies, status reports, real-time
//! commands, and a motion-time estimator shared by the simulator and the
//! progress display.

/// Real-time commands: single bytes acted on at once, outside the line buffer.
pub mod rt {
    pub const STATUS: u8 = b'?';
    pub const FEED_HOLD: u8 = b'!';
    pub const CYCLE_START: u8 = b'~';
    pub const SOFT_RESET: u8 = 0x18;
    pub const JOG_CANCEL: u8 = 0x85;
    /// Spindle (wire) stop override: toggles the spindle off/on, only while
    /// in feed hold; cycle start restores it.
    pub const SPINDLE_STOP: u8 = 0x9E;
}

#[derive(Debug, Clone, PartialEq)]
pub enum State {
    Idle,
    Run,
    /// `Hold:0` = hold complete (stopped), `Hold:1` = still decelerating.
    Hold(u8),
    Jog,
    Alarm,
    Door(u8),
    Check,
    Home,
    Sleep,
    Other(String),
}

impl State {
    fn parse(s: &str) -> State {
        let (name, sub) = s.split_once(':').unwrap_or((s, "0"));
        let sub = sub.parse().unwrap_or(0);
        match name {
            "Idle" => State::Idle,
            "Run" => State::Run,
            "Hold" => State::Hold(sub),
            "Jog" => State::Jog,
            "Alarm" => State::Alarm,
            "Door" => State::Door(sub),
            "Check" => State::Check,
            "Home" => State::Home,
            "Sleep" => State::Sleep,
            other => State::Other(other.to_string()),
        }
    }

    pub fn name(&self) -> String {
        match self {
            State::Hold(n) => format!("Hold:{n}"),
            State::Door(n) => format!("Door:{n}"),
            State::Other(s) => s.clone(),
            s => format!("{s:?}"),
        }
    }
}

/// A `<...>` status report.
#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    pub state: State,
    pub mpos: Option<Vec<f64>>,
    pub wpos: Option<Vec<f64>>,
    /// Work coordinate offset (sent now and then).
    pub wco: Option<Vec<f64>>,
    /// Planner blocks free, RX bytes free.
    pub buffer: Option<(u32, u32)>,
    pub feed: Option<f64>,
    /// Spindle (wire) speed from `FS:feed,speed`; 0 = off.
    pub spindle: Option<f64>,
    /// Active input pins (e.g. `XZ` for limit switches, `P` probe).
    pub pins: String,
}

impl Status {
    /// Machine position, from `MPos` or `WPos + WCO`.
    pub fn machine_pos(&self, last_wco: Option<&[f64]>) -> Option<Vec<f64>> {
        if let Some(m) = &self.mpos {
            return Some(m.clone());
        }
        let w = self.wpos.as_ref()?;
        let o = self.wco.as_deref().or(last_wco)?;
        Some(w.iter().zip(o).map(|(a, b)| a + b).collect())
    }
}

/// One line received from the controller.
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    Ok,
    Error(u32),
    Alarm(u32),
    Status(Status),
    /// Startup banner, e.g. `Grbl 3.9 [FluidNC v4.1.1 ...]`.
    Banner(String),
    /// `[MSG:...]`, `[GC:...]`, `[VER:...]` and other bracketed feedback.
    Feedback(String),
    Other(String),
}

fn floats(s: &str) -> Option<Vec<f64>> {
    s.split(',').map(|v| v.trim().parse().ok()).collect()
}

pub fn parse_status(s: &str) -> Option<Status> {
    let inner = s.strip_prefix('<')?.strip_suffix('>')?;
    let mut parts = inner.split('|');
    let mut st = Status {
        state: State::parse(parts.next()?),
        mpos: None,
        wpos: None,
        wco: None,
        buffer: None,
        feed: None,
        spindle: None,
        pins: String::new(),
    };
    for p in parts {
        let (k, v) = p.split_once(':').unwrap_or((p, ""));
        match k {
            "MPos" => st.mpos = floats(v),
            "WPos" => st.wpos = floats(v),
            "WCO" => st.wco = floats(v),
            "Bf" => {
                if let Some(b) = floats(v)
                    && b.len() == 2
                {
                    st.buffer = Some((b[0] as u32, b[1] as u32));
                }
            }
            "F" => st.feed = floats(v).and_then(|f| f.first().copied()),
            "FS" => {
                let f = floats(v).unwrap_or_default();
                st.feed = f.first().copied();
                st.spindle = f.get(1).copied();
            }
            "Pn" => st.pins = v.to_string(),
            _ => {}
        }
    }
    Some(st)
}

pub fn parse_line(line: &str) -> Reply {
    let l = line.trim();
    if l == "ok" {
        Reply::Ok
    } else if let Some(n) = l.strip_prefix("error:") {
        Reply::Error(n.trim().parse().unwrap_or(0))
    } else if let Some(n) = l.strip_prefix("ALARM:") {
        Reply::Alarm(n.trim().parse().unwrap_or(0))
    } else if let Some(st) = parse_status(l) {
        Reply::Status(st)
    } else if l.starts_with("Grbl") {
        Reply::Banner(l.to_string())
    } else if l.starts_with('[') && l.ends_with(']') {
        Reply::Feedback(l.to_string())
    } else {
        Reply::Other(l.to_string())
    }
}

/// Human text for common grbl error codes.
pub fn error_text(code: u32) -> &'static str {
    match code {
        1 => "G-code words consist of a letter and a value; letter was not found",
        2 => "numeric value format is not valid or missing an expected value",
        3 => "'$' system command was not recognized or supported",
        8 => "'$' command only valid when Idle",
        9 => "G-code locked out during alarm or jog state",
        15 => "jog target exceeds machine travel",
        16 => "jog command with no '=' or contains prohibited g-code",
        20 => "unsupported or invalid g-code command",
        22 => "feed rate has not yet been set or is undefined",
        33 => "motion command target is invalid",
        _ => "see the FluidNC error code list",
    }
}

/// Human text for grbl alarm codes.
pub fn alarm_text(code: u32) -> &'static str {
    match code {
        1 => "hard limit triggered: position lost, re-home",
        2 => "soft limit: a motion target exceeds machine travel",
        3 => "reset while in motion: position may be lost, re-home",
        6..=9 => "homing failed",
        10 => "E-stop or limit at startup",
        _ => "see the FluidNC alarm list",
    }
}

/// Strip comments (`; ...` and `( ... )`) and whitespace; `None` if empty.
pub fn clean_line(raw: &str) -> Option<String> {
    let mut out = String::with_capacity(raw.len());
    let mut in_paren = false;
    for c in raw.chars() {
        match c {
            ';' if !in_paren => break,
            '(' => in_paren = true,
            ')' => in_paren = false,
            c if !in_paren => out.push(c),
            _ => {}
        }
    }
    let t = out.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// Letter/number words of a G-code line (`G1 X1.5` → `[('G',1.0), ('X',1.5)]`).
pub fn words(line: &str) -> Vec<(char, f64)> {
    let b = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = (b[i] as char).to_ascii_uppercase();
        if c.is_ascii_alphabetic() {
            let mut j = i + 1;
            while j < b.len() && (b[j] == b' ') {
                j += 1;
            }
            let start = j;
            while j < b.len()
                && (b[j].is_ascii_digit() || b[j] == b'.' || b[j] == b'-' || b[j] == b'+')
            {
                j += 1;
            }
            if let Ok(v) = line[start..j].parse::<f64>() {
                out.push((c, v));
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    out
}

/// Tracks modal state through a program to estimate how long each line
/// takes (G93 inverse time, G94 feed, G0 at the rapid rate, G4 dwells).
#[derive(Debug, Clone)]
pub struct Motion {
    pub axes: [char; 4],
    pub pos: [f64; 4],
    pub absolute: bool,
    pub inverse_time: bool,
    pub feed: f64,
    pub rapid: f64,
    pub motion: u8,
}

impl Motion {
    pub fn new(axes: [char; 4], rapid: f64) -> Motion {
        Motion {
            axes,
            pos: [0.0; 4],
            absolute: true,
            inverse_time: false,
            feed: 0.0,
            rapid,
            motion: 0,
        }
    }

    /// Apply one line; returns its duration (seconds) and the target if it
    /// moves. Inverse-time F applies to this line only.
    pub fn apply(&mut self, line: &str) -> (f64, Option<[f64; 4]>) {
        let ws = words(line);
        let mut f_word = None;
        let mut dwell = None;
        let mut target = self.pos;
        let mut has_axis = false;
        for &(c, v) in &ws {
            match c {
                'G' => match v as u32 {
                    0 => self.motion = 0,
                    1 => self.motion = 1,
                    4 => dwell = Some(0.0),
                    90 => self.absolute = true,
                    91 => self.absolute = false,
                    93 => self.inverse_time = true,
                    94 => self.inverse_time = false,
                    _ => {}
                },
                'F' => f_word = Some(v),
                'P' if dwell.is_some() => dwell = Some(v),
                _ => {
                    if let Some(k) = self.axes.iter().position(|a| *a == c) {
                        has_axis = true;
                        target[k] = if self.absolute { v } else { self.pos[k] + v };
                    }
                }
            }
        }
        if let Some(p) = dwell {
            return (p, None);
        }
        if !self.inverse_time
            && let Some(f) = f_word
        {
            self.feed = f;
        }
        if !has_axis {
            return (0.0, None);
        }
        let dist = (0..4)
            .map(|k| (target[k] - self.pos[k]).powi(2))
            .sum::<f64>()
            .sqrt();
        let minutes = if self.motion == 0 {
            dist / self.rapid
        } else if self.inverse_time {
            f_word.map_or(0.0, |f| if f > 0.0 { 1.0 / f } else { 0.0 })
        } else if self.feed > 0.0 {
            dist / self.feed
        } else {
            0.0
        };
        self.pos = target;
        (minutes * 60.0, Some(target))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fluidnc_status_reports() {
        let s = parse_status("<Run|MPos:1.000,2.500,-3.000,4.000|Bf:15,127|FS:180,1000|Pn:XZ>")
            .unwrap();
        assert_eq!(s.state, State::Run);
        assert_eq!(s.mpos, Some(vec![1.0, 2.5, -3.0, 4.0]));
        assert_eq!(s.buffer, Some((15, 127)));
        assert_eq!(s.feed, Some(180.0));
        assert_eq!(s.spindle, Some(1000.0));
        assert_eq!(s.pins, "XZ");
        let h = parse_status("<Hold:1|WPos:0,0,0,0|WCO:10,0,10,0>").unwrap();
        assert_eq!(h.state, State::Hold(1));
        assert_eq!(h.machine_pos(None), Some(vec![10.0, 0.0, 10.0, 0.0]));
        let w = parse_status("<Idle|WPos:1,1,1,1>").unwrap();
        assert_eq!(
            w.machine_pos(Some(&[1.0, 2.0, 3.0, 4.0])),
            Some(vec![2.0, 3.0, 4.0, 5.0])
        );
        assert!(parse_status("<Idle|MPos:a,b>").unwrap().mpos.is_none());
    }

    #[test]
    fn classifies_replies() {
        assert_eq!(parse_line("ok\r"), Reply::Ok);
        assert_eq!(parse_line("error:20"), Reply::Error(20));
        assert_eq!(parse_line("ALARM:2"), Reply::Alarm(2));
        assert!(matches!(
            parse_line("Grbl 3.9 [FluidNC v4.1.1 '$' for help]"),
            Reply::Banner(_)
        ));
        assert!(matches!(
            parse_line("[MSG:INFO: Homed]"),
            Reply::Feedback(_)
        ));
        assert!(matches!(
            parse_line("<Idle|MPos:0,0,0,0>"),
            Reply::Status(_)
        ));
        assert!(matches!(parse_line("hello"), Reply::Other(_)));
    }

    #[test]
    fn strips_comments() {
        assert_eq!(clean_line("G1 X1 ; move"), Some("G1 X1".into()));
        assert_eq!(clean_line("(note) G4 P2"), Some("G4 P2".into()));
        assert_eq!(clean_line("; only a comment"), None);
        assert_eq!(clean_line("   "), None);
    }

    #[test]
    fn estimates_g93_g94_rapids_and_dwells() {
        let mut m = Motion::new(['X', 'Y', 'Z', 'A'], 800.0);
        assert_eq!(m.apply("G21 G90 G94").0, 0.0);
        let (t, _) = m.apply("G1 Y30 A40 F800"); // 50 mm at 800 mm/min
        assert!((t - 3.75).abs() < 1e-9);
        assert_eq!(m.apply("G4 P2.000").0, 2.0);
        m.apply("G93");
        let (t, p) = m.apply("G1 X10 Y30 Z10 A40 F4.0"); // 1/4 min
        assert!((t - 15.0).abs() < 1e-9);
        assert_eq!(p, Some([10.0, 30.0, 10.0, 40.0]));
        m.apply("G94");
        let (t, _) = m.apply("G0 X0"); // rapid 10 mm
        assert!((t - 0.75).abs() < 1e-9);
        m.apply("G91");
        let (_, p) = m.apply("G1 X5 F100");
        assert_eq!(p.unwrap()[0], 5.0);
    }

    #[test]
    fn splits_words() {
        assert_eq!(
            words("G1X-1.5 y2"),
            vec![('G', 1.0), ('X', -1.5), ('Y', 2.0)]
        );
        // "J=" has no number, so it is skipped.
        assert_eq!(
            words("$J=G91 X10 F500"),
            vec![('G', 91.0), ('X', 10.0), ('F', 500.0)]
        );
    }
}
