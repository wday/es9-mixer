//! Conversions between wire values and human units.
//!
//! Two independent gain scales exist. The **raw mix** is a 16-bit linear gain applied
//! by the mixer hardware. The **macro mix** is a 7-bit, MIDI-compatible scale that the
//! module expands into raw gains; it is what MIDI CC control addresses.

/// Raw gain representing unity (0 dB).
pub const RAW_UNITY: u16 = 0x2000;

/// Largest raw gain the config tool will send, equal to +12 dB.
pub const RAW_MAX: u16 = 0x7FFF;

/// Macro level that the config tool's double-click sets, equal to 0 dB.
pub const MACRO_UNITY: u8 = 103;

/// Level below which a gain is treated as silence rather than a dB value.
pub const SILENCE_DB: f32 = -72.0;

/// Largest output DC offset the module accepts, either side of zero.
///
/// Taken from the official configuration tool, whose slider runs `-3176..=3176`. The
/// manual gives no units and the tool displays the bare number, so no voltage conversion
/// is offered here rather than inventing one.
pub const DC_OFFSET_MAX: i16 = 3176;

/// The value to **write** for pan centre.
///
/// Pan is written to CC `(m+1)*8 + ch` but is not stored there. Measured on hardware
/// 2026-08-30 (checklist H3): the module keeps pan in the **aux byte of the level cell**,
/// and stores `written - 1` (saturating at 0). So writing and reading use biases that
/// differ by one, and neither is a mistake.
///
/// This vindicates the reference configuration tool, which this project had recorded as
/// buggy here: it writes `64 + pan` and reads `byte - 63`, and those compose exactly
/// because the device subtracts one in between.
pub const PAN_CENTRE: u8 = 64;

/// The **stored** aux byte that means pan centre — one less than [`PAN_CENTRE`].
pub const PAN_CENTRE_STORED: u8 = 63;

/// Largest pan offset either side of centre.
pub const PAN_MAX: i8 = 63;

/// Converts the stored aux byte to a pan offset from centre, negative left.
///
/// Takes the byte from the *level* cell's aux, not from the CC pan was written to.
pub fn stored_to_pan(aux: u8) -> i8 {
    (i16::from(aux) - i16::from(PAN_CENTRE_STORED)).clamp(i16::from(-PAN_MAX), i16::from(PAN_MAX))
        as i8
}

/// Converts a pan offset from centre to the value to write to the pan CC.
pub fn pan_to_macro(pan: i8) -> u8 {
    (i16::from(pan.clamp(-PAN_MAX, PAN_MAX)) + i16::from(PAN_CENTRE)) as u8
}

/// What the module will store for a given written pan value.
///
/// Only needed by the offline mock, which has to reproduce the device's off-by-one for
/// the round trip to behave the way the hardware does.
pub fn written_to_stored(written: u8) -> u8 {
    written.saturating_sub(1)
}

/// Converts a raw 16-bit mixer gain to decibels.
///
/// Returns [`f32::NEG_INFINITY`] for zero, and saturates at +12 dB.
pub fn raw_to_db(v: u16) -> f32 {
    if v == 0 {
        return f32::NEG_INFINITY;
    }
    if v >= RAW_MAX {
        return 12.0;
    }
    6.0 * (f32::from(v) / 8192.0).log2()
}

/// Converts decibels to a raw 16-bit mixer gain, saturating at +12 dB.
///
/// Anything below [`SILENCE_DB`] becomes zero, matching the config tool.
pub fn db_to_raw(db: f32) -> u16 {
    if db < SILENCE_DB {
        return 0;
    }
    let v = (f32::from(RAW_UNITY) * (db / 6.0).exp2()).round();
    if v >= f32::from(RAW_MAX) {
        RAW_MAX
    } else {
        v as u16
    }
}

/// Converts a 7-bit macro level to decibels.
///
/// The scale is piecewise: coarse below -24 dB, then 0.5 dB per step above it. This
/// puts the usable resolution where mixing decisions are actually made.
pub fn macro_to_db(v: u8) -> f32 {
    if v == 0 {
        return f32::NEG_INFINITY;
    }
    if v >= 55 {
        -24.0 + 0.5 * (f32::from(v) - 55.0)
    } else {
        -72.0 + (f32::from(v) - 1.0) * (72.0 - 24.0) / (55.0 - 1.0)
    }
}

/// Converts decibels to a 7-bit macro level.
pub fn db_to_macro(db: f32) -> u8 {
    if db < SILENCE_DB {
        return 0;
    }
    let v = if db >= -24.0 {
        ((db + 24.0) * 2.0 + 55.0).round()
    } else {
        ((db + 72.0) * (55.0 - 1.0) / (72.0 - 24.0) + 1.0).round()
    };
    v.clamp(0.0, 127.0) as u8
}

/// Converts a macro level to the raw gain the module will run.
///
/// Spike S1 established that the macro layer drives the raw matrix, so this is the
/// conversion the hardware performs internally. Composed from the documented curves
/// rather than measured, and it is **not exact**: unity (macro 103) reproduces the
/// hardware's `0x2000` precisely, but macro 100 computes 6889 where a real module settled
/// at 6832 — about 0.07 dB out. See open question Q9 in `docs/es9-sysex-protocol.md`.
///
/// Nothing in the application depends on this: the module performs the real conversion
/// and reports the result. It exists so the offline mock behaves like the hardware.
pub fn macro_to_raw(v: u8) -> u16 {
    db_to_raw(macro_to_db(v))
}

/// Converts an EQ frequency word to hertz. The scale is logarithmic, 10 Hz to 22 kHz.
pub fn eq_freq_hz(v: u32) -> f32 {
    let min = 10.0_f32;
    let mult = (22000.0_f32 / min).ln() / 32767.0;
    min * (mult * v as f32).exp()
}

/// Converts an EQ Q word to a Q value. Logarithmic, 0.1 to 18.
pub fn eq_q(v: u32) -> f32 {
    let min = 0.1_f32;
    let mult = (18.0_f32 / min).ln() / 32767.0;
    min * (mult * v as f32).exp()
}

/// Converts an EQ gain word to decibels. Linear, up to 15 dB.
pub fn eq_gain_db(v: i32) -> f32 {
    v as f32 * 15.0 / 32767.0
}
