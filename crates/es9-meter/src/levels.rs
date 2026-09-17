//! What one metered channel looks like once it leaves the audio thread.

use crate::Meter;

/// One channel's levels, in the units the meter is drawn in.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Levels {
    /// Peak, dBFS.
    pub peak_db: f32,
    /// RMS, dBFS.
    pub rms_db: f32,
    /// Held peak, dBFS.
    pub hold_db: f32,
    /// Whether the channel has clipped since the hold was last reset.
    pub clipped: bool,
    /// Smoothed mean sample value: the channel's DC offset, linear and signed.
    pub dc: f32,
}

impl Levels {
    /// Reads a meter's current state.
    pub fn of(meter: &Meter) -> Self {
        Self {
            peak_db: meter.peak_db(),
            rms_db: meter.rms_db(),
            hold_db: crate::linear_to_db(meter.hold),
            clipped: meter.clipped,
            dc: meter.dc,
        }
    }
}

/// A snapshot of every metered channel, with the stream it came from.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct MeterSnapshot {
    /// Per-channel levels, in capture-channel order.
    pub channels: Vec<Levels>,
    /// Stream sample rate.
    pub sample_rate: u32,
    /// Host the stream was opened on, e.g. `"ASIO"`.
    pub host: String,
}
