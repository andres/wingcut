//! Byte links to a controller: USB serial, FluidNC's WebSocket, plain TCP
//! (FluidNC's telnet port), or the built-in simulator.

use anyhow::{Context, Result};
use std::io::{self, ErrorKind, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// A bidirectional byte stream to a controller.
pub trait Link: Send {
    /// Write all of `data`.
    fn send(&mut self, data: &[u8]) -> io::Result<()>;
    /// Read what is available, waiting up to about `timeout`.
    /// `Ok(0)` means nothing arrived; an error means the link is gone.
    fn recv(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<usize>;
    /// Short description for logs and the UI.
    fn describe(&self) -> String;
}

fn timed_out(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        ErrorKind::TimedOut | ErrorKind::WouldBlock | ErrorKind::Interrupted
    )
}

/// Names of the serial ports on this computer (e.g. `/dev/ttyUSB0`, `COM3`).
pub fn serial_ports() -> Vec<String> {
    serialport::available_ports()
        .map(|v| v.into_iter().map(|p| p.port_name).collect())
        .unwrap_or_default()
}

pub struct SerialLink {
    port: Box<dyn serialport::SerialPort>,
    name: String,
}

impl SerialLink {
    pub fn open(path: &str, baud: u32) -> Result<SerialLink> {
        let mut port = serialport::new(path, baud)
            .timeout(Duration::from_millis(10))
            // Many ESP32 boards reset when DTR/RTS toggle on open.
            .dtr_on_open(false)
            .open()
            .with_context(|| {
                let hint = if cfg!(target_os = "linux") {
                    " (on Linux, your user may need to be in the 'dialout' group: sudo usermod -aG dialout $USER, then log out and in)"
                } else {
                    ""
                };
                format!("opening serial port {path}{hint}")
            })?;
        let _ = port.write_request_to_send(false);
        Ok(SerialLink {
            port,
            name: format!("{path} @ {baud}"),
        })
    }
}

impl Link for SerialLink {
    fn send(&mut self, data: &[u8]) -> io::Result<()> {
        self.port.write_all(data)?;
        self.port.flush()
    }

    fn recv(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<usize> {
        if self.port.timeout() != timeout {
            self.port.set_timeout(timeout)?;
        }
        match self.port.read(buf) {
            Ok(n) => Ok(n),
            Err(e) if timed_out(&e) => Ok(0),
            Err(e) => Err(e),
        }
    }

    fn describe(&self) -> String {
        format!("serial {}", self.name)
    }
}

fn tcp_connect(host: &str, port: u16) -> Result<TcpStream> {
    let addr = (host, port)
        .to_socket_addrs()
        .with_context(|| format!("resolving {host}"))?
        .next()
        .with_context(|| format!("no address for {host}"))?;
    let s = TcpStream::connect_timeout(&addr, Duration::from_secs(5))
        .with_context(|| format!("connecting to {host}:{port}"))?;
    s.set_nodelay(true)?;
    Ok(s)
}

/// Raw TCP, e.g. FluidNC's telnet server (port 23).
pub struct TcpLink {
    s: TcpStream,
    name: String,
}

impl TcpLink {
    pub fn open(host: &str, port: u16) -> Result<TcpLink> {
        Ok(TcpLink {
            s: tcp_connect(host, port)?,
            name: format!("{host}:{port}"),
        })
    }
}

impl Link for TcpLink {
    fn send(&mut self, data: &[u8]) -> io::Result<()> {
        self.s.write_all(data)
    }

    fn recv(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<usize> {
        self.s
            .set_read_timeout(Some(timeout.max(Duration::from_millis(1))))?;
        match self.s.read(buf) {
            Ok(0) => Err(io::Error::new(
                ErrorKind::ConnectionAborted,
                "connection closed",
            )),
            Ok(n) => Ok(n),
            Err(e) if timed_out(&e) => Ok(0),
            Err(e) => Err(e),
        }
    }

    fn describe(&self) -> String {
        format!("tcp {}", self.name)
    }
}

/// FluidNC's WebSocket console (the WebUI's, usually port 81). Console
/// output arrives as binary frames; text frames such as `CURRENT_ID:0` or
/// `PING:...` are WebUI housekeeping and are dropped.
pub struct WsLink {
    ws: tungstenite::WebSocket<TcpStream>,
    pending: Vec<u8>,
    name: String,
}

impl WsLink {
    pub fn open(host: &str, port: u16) -> Result<WsLink> {
        let stream = tcp_connect(host, port)?;
        let url = format!("ws://{host}:{port}/");
        let (ws, _) = tungstenite::client::client(url.as_str(), stream)
            .map_err(|e| anyhow::anyhow!("WebSocket handshake with {url} failed: {e}"))?;
        Ok(WsLink {
            ws,
            pending: Vec::new(),
            name: url,
        })
    }
}

fn ws_err(e: tungstenite::Error) -> io::Error {
    match e {
        tungstenite::Error::Io(e) => e,
        other => io::Error::new(ErrorKind::ConnectionAborted, other.to_string()),
    }
}

/// WebUI housekeeping messages, not console output.
fn is_webui_text(t: &str) -> bool {
    [
        "CURRENT_ID:",
        "ACTIVE_ID:",
        "PING:",
        "DHT:",
        "currentID:",
        "activeID:",
    ]
    .iter()
    .any(|p| t.starts_with(p))
}

impl Link for WsLink {
    fn send(&mut self, data: &[u8]) -> io::Result<()> {
        let text = String::from_utf8_lossy(data).into_owned();
        self.ws
            .send(tungstenite::Message::text(text))
            .map_err(ws_err)
    }

    fn recv(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<usize> {
        if self.pending.is_empty() {
            self.ws
                .get_mut()
                .set_read_timeout(Some(timeout.max(Duration::from_millis(1))))?;
            match self.ws.read() {
                Ok(tungstenite::Message::Binary(b)) => self.pending.extend_from_slice(&b),
                Ok(tungstenite::Message::Text(t)) if !is_webui_text(t.as_str()) => {
                    self.pending.extend_from_slice(t.as_bytes())
                }
                Ok(tungstenite::Message::Close(_)) => {
                    return Err(io::Error::new(
                        ErrorKind::ConnectionAborted,
                        "WebSocket closed",
                    ));
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(e)) if timed_out(&e) => {}
                Err(e) => return Err(ws_err(e)),
            }
        }
        let n = self.pending.len().min(buf.len());
        buf[..n].copy_from_slice(&self.pending[..n]);
        self.pending.drain(..n);
        Ok(n)
    }

    fn describe(&self) -> String {
        format!("websocket {}", self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tcp_link_round_trip() {
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let t = std::thread::spawn(move || {
            let (mut s, _) = server.accept().unwrap();
            let mut b = [0u8; 3];
            s.read_exact(&mut b).unwrap();
            s.write_all(b"ok\r\n").unwrap();
            b
        });
        let mut l = TcpLink::open("127.0.0.1", port).unwrap();
        l.send(b"$I\n").unwrap();
        let mut got = Vec::new();
        let mut buf = [0u8; 16];
        for _ in 0..200 {
            let n = l.recv(&mut buf, Duration::from_millis(10)).unwrap();
            got.extend_from_slice(&buf[..n]);
            if got.ends_with(b"\n") {
                break;
            }
        }
        assert_eq!(&t.join().unwrap(), b"$I\n");
        assert_eq!(got, b"ok\r\n");
    }

    #[test]
    fn websocket_link_round_trip_drops_webui_noise() {
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let t = std::thread::spawn(move || {
            let (s, _) = server.accept().unwrap();
            let mut ws = tungstenite::accept(s).unwrap();
            ws.send(tungstenite::Message::text("CURRENT_ID:0")).unwrap();
            let msg = ws.read().unwrap();
            ws.send(tungstenite::Message::binary(b"ok\r\n".to_vec()))
                .unwrap();
            msg.into_text().unwrap().to_string()
        });
        let mut l = WsLink::open("127.0.0.1", port).unwrap();
        l.send(b"?").unwrap();
        let mut got = Vec::new();
        let mut buf = [0u8; 16];
        for _ in 0..300 {
            let n = l.recv(&mut buf, Duration::from_millis(10)).unwrap();
            got.extend_from_slice(&buf[..n]);
            if got.ends_with(b"\n") {
                break;
            }
        }
        assert_eq!(t.join().unwrap(), "?");
        assert_eq!(got, b"ok\r\n");
    }

    #[test]
    fn missing_serial_port_gives_a_clear_error() {
        let e = SerialLink::open("/dev/does-not-exist", 115200)
            .err()
            .unwrap();
        assert!(format!("{e:#}").contains("opening serial port /dev/does-not-exist"));
    }
}
