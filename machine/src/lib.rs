//! Machine control for wingcut: talk to a FluidNC (grbl) board over USB
//! serial, WebSocket or TCP, stream cut programs safely, drive a Riden
//! RD60xx wire supply, and simulate both for testing without hardware.

pub mod controller;
pub mod grbl;
pub mod link;
pub mod riden;
pub mod sim;

pub use controller::{Command, Controller, Event, Job, Options};
pub use link::{Link, SerialLink, TcpLink, WsLink, serial_ports};
pub use sim::{Sim, SimConfig, SimLink};
