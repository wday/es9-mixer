//! Meter ballistics, and the level readout the UI draws from them.
//!
//! Metering is host-side: the module has no audio-level telemetry in its SysEx protocol,
//! only DSP load. The ES-9 can route any input, bus or mix output to a USB capture
//! channel, so the host can meter any point that is worth spending a capture channel on.
//!
//! Nothing here opens a stream or knows what an audio backend is. It takes blocks of
//! samples and produces the numbers a meter draws, which is the same arithmetic whether
//! the samples arrived over ASIO, CoreAudio or a test fixture.

#![forbid(unsafe_code)]

mod ballistics;
mod levels;

pub use ballistics::{Meter, linear_to_db};
pub use levels::{Levels, MeterSnapshot};
