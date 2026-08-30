//! Codec for the Expert Sleepers ES-9 MIDI SysEx protocol.
//!
//! Targets **firmware 1.3**. The configuration dump format changed between firmware 1.2
//! and 1.3 while the version field inside the payload stayed at `3`, so the two can only
//! be told apart by command byte; a 1.2 dump is rejected with
//! [`decode::DecodeError::FirmwareTooOld`] rather than misparsed.
//!
//! This crate is pure: no I/O, no MIDI, no platform dependencies. See
//! `docs/es9-sysex-protocol.md` for the protocol itself and its open questions.

#![forbid(unsafe_code)]

pub mod ccmap;
pub mod config;
pub mod dcblock;
pub mod decode;
pub mod encode;
pub mod frame;
pub mod links;
pub mod scales;
pub mod tables;

pub use config::Config;
pub use decode::{Incoming, decode};
pub use links::Links;
