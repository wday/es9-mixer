//! MIDI transport for the ES-9.
//!
//! Wraps `midir`, reassembles SysEx, and logs every message in both directions.

#![forbid(unsafe_code)]

pub mod assembler;
pub mod link;

pub use assembler::SysexAssembler;
pub use es9_device::monitor::{Direction, Entry, Monitor};
pub use link::{MidiError, MidiLink, PortInfo, find_es9, list_ports};
