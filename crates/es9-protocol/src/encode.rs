//! Building messages to send to the ES-9.

use crate::config::{
    Config, DUMP_WORDS, W_CAPTURE, W_DC, W_EQ, W_HPF, W_LINKS_HI, W_LINKS_LO, W_MACRO, W_MIDI,
    W_OPTIONS, W_OUTPUTS, W_RAW_MIX, W_RESERVED, W_SMOOTHING, W_VERSION,
};
use crate::frame::{self, push21};

/// Which flash slot a save or restore targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlashSlot {
    /// Loaded when the module powers up with no USB connection.
    Standalone = 0,
    /// Loaded when a USB host is present.
    Hosted = 1,
}

/// Sends a text message to the module.
pub fn message(text: &str) -> Vec<u8> {
    let mut v = frame::begin(0x02);
    v.extend(text.bytes().filter(|b| b & 0x80 == 0));
    frame::end(v)
}

fn simple(cmd: u8) -> Vec<u8> {
    frame::end(frame::begin(cmd))
}

/// Requests the version string. Answered with a `32H` message.
pub fn request_version() -> Vec<u8> {
    simple(0x22)
}

/// Requests a full configuration dump.
pub fn request_config() -> Vec<u8> {
    simple(0x23)
}

/// Requests the mix state.
pub fn request_mix() -> Vec<u8> {
    simple(0x2A)
}

/// Requests DSP usage.
pub fn request_usage() -> Vec<u8> {
    simple(0x2B)
}

/// Requests the current sample rate.
pub fn request_sample_rate() -> Vec<u8> {
    simple(0x2C)
}

/// Saves the current state to a flash slot. Answered with `"OK"`.
pub fn save_to_flash(slot: FlashSlot) -> Vec<u8> {
    frame::end({
        let mut v = frame::begin(0x24);
        v.push(slot as u8);
        v
    })
}

/// Restores state from a flash slot.
///
/// Connecting over USB forces the module into hosted mode, so editing the standalone
/// configuration means restoring [`FlashSlot::Standalone`] **first**. Skipping that step
/// silently edits hosted state instead.
pub fn restore_from_flash(slot: FlashSlot) -> Vec<u8> {
    frame::end({
        let mut v = frame::begin(0x25);
        v.push(slot as u8);
        v
    })
}

/// Resets the **current** configuration to one of the two factory default sets.
///
/// `Hosted` sets mixer inputs to the USB channels and enables S/PDIF; `Standalone` sets
/// them to the analogue inputs and enables the second mixer block.
///
/// This does **not** touch either flash slot — the manual is explicit that resetting a
/// saved configuration means resetting the current one and then saving it. The parameter
/// is easy to miss: the reference tool sends it, and without it the module is being told
/// to reset to an unspecified default.
pub fn reset_to_defaults(target: FlashSlot) -> Vec<u8> {
    frame::end({
        let mut v = frame::begin(0x26);
        v.push(target as u8);
        v
    })
}

/// Sets the DC-blocking filter bitmap, one bit per input pair.
///
/// Bits 0-6 address the seven pairs in [`crate::dcblock::PAIRS`]. See that module for why
/// they are pairs and why they are not sequential.
pub fn set_hpf(bits: u8) -> Vec<u8> {
    frame::end({
        let mut v = frame::begin(0x31);
        v.push(bits & 0x7F);
        v
    })
}

/// Sets the global options word.
pub fn set_options(options: crate::config::Options) -> Vec<u8> {
    frame::end({
        let mut v = frame::begin(0x32);
        v.push(options.to_bits() as u8);
        v
    })
}

/// Enables or disables a stereo link, identified by its bit position in the bitmap.
pub fn set_link(index: u8, on: bool) -> Vec<u8> {
    frame::end({
        let mut v = frame::begin(0x33);
        v.push(index);
        v.push(u8::from(on));
        v
    })
}

/// Sets a macro mix control.
///
/// `cc` is the MIDI CC number for the control, which is the same index the module uses
/// internally. Resolve it with [`crate::ccmap::CcMap`] rather than computing it by hand,
/// because stereo links change the mapping.
pub fn set_macro(cc: u8, value: u8) -> Vec<u8> {
    frame::end({
        let mut v = frame::begin(0x34);
        v.push(cc & 0x7F);
        v.push(value & 0x7F);
        v
    })
}

/// Sets the MIDI channels used for CC control. Channel `0` disables that port.
pub fn set_midi_channels(usb: u8, din: u8) -> Vec<u8> {
    frame::end({
        let mut v = frame::begin(0x35);
        v.push(usb & 0x7F);
        v.push(din & 0x7F);
        v
    })
}

/// Sets an output DC offset.
pub fn set_dc_offset(channel: u8, offset: i16) -> Vec<u8> {
    frame::end({
        let mut v = frame::begin(0x36);
        v.push(channel);
        push21(&mut v, frame::to_unsigned16(i32::from(offset)));
        v
    })
}

/// Sets one band of input EQ. `block` is 0 or 1, `channel` 0-7, `band` 0-3.
pub fn set_filter(block: u8, channel: u8, band: u8, eq: crate::config::EqBand) -> Vec<u8> {
    frame::end({
        let mut v = frame::begin(0x39);
        v.push(block * 8 + channel);
        v.push(band);
        v.push((eq.kind << 1) | u8::from(eq.enabled));
        push21(&mut v, eq.freq);
        push21(&mut v, eq.q);
        push21(&mut v, frame::to_unsigned16(eq.gain));
        v
    })
}

/// Enables or disables gain smoothing for one mix.
pub fn set_smoothing(mix: u8, on: bool) -> Vec<u8> {
    frame::end({
        let mut v = frame::begin(0x3A);
        v.push(mix);
        v.push(u8::from(on));
        v
    })
}

/// Sets the capture routing for one block, as eight wire bytes.
pub fn set_capture(block: u8, sources: [u8; 8]) -> Vec<u8> {
    frame::end({
        let mut v = frame::begin(0x40 + block);
        v.extend_from_slice(&sources);
        v
    })
}

/// Sets the output routing for one block, as eight wire bytes.
pub fn set_outputs(block: u8, destinations: [u8; 8]) -> Vec<u8> {
    frame::end({
        let mut v = frame::begin(0x50 + block);
        v.extend_from_slice(&destinations);
        v
    })
}

/// Sets one raw mixer gain.
pub fn set_raw_mix(mix: u8, channel: u8, value: u16) -> Vec<u8> {
    frame::end({
        let mut v = frame::begin(0x60 + (mix & 0x0F));
        v.push(channel);
        push21(&mut v, u32::from(value));
        v
    })
}

/// Serialises a configuration into the 768-word dump body.
pub fn config_words(c: &Config) -> Vec<u32> {
    let mut w = vec![0u32; DUMP_WORDS];
    w[W_VERSION] = c.version;
    w[W_HPF] = c.hpf;
    for block in 0..4 {
        for ch in 0..8 {
            w[W_CAPTURE + block * 8 + ch] = u32::from(c.capture[block][ch]);
            w[W_OUTPUTS + block * 8 + ch] = u32::from(c.outputs[block][ch]);
        }
    }
    for mix in 0..16 {
        for ch in 0..8 {
            w[W_RAW_MIX + mix * 8 + ch] = u32::from(c.raw_mix[mix][ch]);
        }
    }
    w[W_MACRO..W_MACRO + 256].copy_from_slice(&c.macro_words[..]);
    w[W_OPTIONS] = c.options.to_bits();
    w[W_LINKS_LO] = c.links.0 & 0xFFFF;
    w[W_LINKS_HI] = (c.links.0 >> 16) & 0xFFFF;
    w[W_MIDI] = (u32::from(c.midi_usb) << 8) | u32::from(c.midi_din);
    for i in 0..8 {
        w[W_DC + i] = frame::to_unsigned16(i32::from(c.dc_offsets[i]));
    }
    let mut p = W_EQ;
    for block in 0..2 {
        for ch in 0..8 {
            for band in 0..4 {
                let e = c.eq[block][ch][band];
                w[p] = (u32::from(e.kind) << 1) | u32::from(e.enabled);
                w[p + 1] = e.freq;
                w[p + 2] = e.q;
                w[p + 3] = frame::to_unsigned16(e.gain);
                p += 4;
            }
        }
    }
    w[W_SMOOTHING] = u32::from(c.smoothing);
    w[W_RESERVED..DUMP_WORDS].copy_from_slice(&c.reserved[..]);
    w
}

/// Builds a complete `08H` configuration dump, as the module would send it.
///
/// Used by the mock device and by round-trip tests.
pub fn config_dump(c: &Config) -> Vec<u8> {
    let mut v = frame::begin(0x08);
    v.push(0);
    v.push(0);
    for word in config_words(c) {
        push21(&mut v, word);
    }
    frame::end(v)
}

/// Number of chunks a configuration upload is split into.
pub const UPLOAD_CHUNKS: usize = 24;
/// Words carried by each upload chunk.
pub const UPLOAD_WORDS_PER_CHUNK: usize = DUMP_WORDS / UPLOAD_CHUNKS;

/// Splits a configuration into the 24 `09H` chunks the module expects.
///
/// There is no acknowledgement or flow control, so a caller should read the
/// configuration back afterwards and compare rather than assume the upload landed.
pub fn config_upload(c: &Config) -> Vec<Vec<u8>> {
    let words = config_words(c);
    (0..UPLOAD_CHUNKS)
        .map(|i| {
            let mut v = frame::begin(0x09);
            v.push(i as u8);
            v.push(0);
            let start = i * UPLOAD_WORDS_PER_CHUNK;
            for word in &words[start..start + UPLOAD_WORDS_PER_CHUNK] {
                push21(&mut v, *word);
            }
            frame::end(v)
        })
        .collect()
}
