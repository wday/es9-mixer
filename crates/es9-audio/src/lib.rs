//! Audio capture and metering for the ES-9.
//!
//! Metering is host-side: the module has no audio-level telemetry in its SysEx protocol,
//! only DSP load. The ES-9 can route any input, bus or mix output to a USB capture
//! channel, so the host can meter any point that is worth spending a capture channel on.

#![forbid(unsafe_code)]

pub mod capture;
pub mod meter;
