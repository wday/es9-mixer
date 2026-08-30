//! The device configuration as carried by a `08H` dump (firmware 1.3).
//!
//! Every field in a 1.3 dump is a 21-bit word — including single-bit flags — so the
//! layout is expressed in words, not bytes.

use crate::links::Links;
use crate::tables::{Destination, Source};

/// Total words in a firmware 1.3 configuration dump.
pub const DUMP_WORDS: usize = 768;

/// Config layout version this crate understands.
pub const SUPPORTED_VERSION: u32 = 3;

pub(crate) const W_VERSION: usize = 0;
pub(crate) const W_HPF: usize = 1;
pub(crate) const W_CAPTURE: usize = 2;
pub(crate) const W_OUTPUTS: usize = 34;
pub(crate) const W_RAW_MIX: usize = 66;
pub(crate) const W_MACRO: usize = 194;
pub(crate) const W_OPTIONS: usize = 450;
pub(crate) const W_LINKS_LO: usize = 451;
pub(crate) const W_LINKS_HI: usize = 452;
pub(crate) const W_MIDI: usize = 453;
pub(crate) const W_DC: usize = 454;
pub(crate) const W_EQ: usize = 462;
pub(crate) const W_SMOOTHING: usize = 718;
pub(crate) const W_RESERVED: usize = 719;

/// Global options word.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    /// Use a second mixer in block 3 instead of the S/PDIF ports.
    pub mixer2: bool,
    /// Turn the MIDI output into a thru. Standalone mode only.
    pub midi_thru: bool,
}

impl Options {
    /// Decodes the options word.
    pub const fn from_bits(v: u32) -> Self {
        Self {
            mixer2: v & 1 != 0,
            midi_thru: v & 2 != 0,
        }
    }

    /// Encodes to the options word.
    pub const fn to_bits(self) -> u32 {
        (self.mixer2 as u32) | ((self.midi_thru as u32) << 1)
    }
}

/// One band of input EQ.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EqBand {
    /// Whether the band is active.
    pub enabled: bool,
    /// Filter type. 0-7; gain applies to types 4-6, Q to types 2, 3 and 6.
    pub kind: u8,
    /// Frequency word; convert with [`crate::scales::eq_freq_hz`].
    pub freq: u32,
    /// Q word; convert with [`crate::scales::eq_q`].
    pub q: u32,
    /// Gain word, signed; convert with [`crate::scales::eq_gain_db`].
    pub gain: i32,
}

/// A complete ES-9 configuration.
///
/// Routing is held as raw wire bytes rather than decoded enums so that an unrecognised
/// value survives a load/save cycle unchanged instead of being silently dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Layout version. Must be [`SUPPORTED_VERSION`].
    pub version: u32,
    /// DC-blocking filter bitmap, one bit per *input pair*.
    ///
    /// Bits 0-6 address the seven pairs in [`crate::dcblock::PAIRS`] — not inputs 1-7.
    /// The filters switch in pairs, and the pairs are not sequential.
    pub hpf: u32,
    /// Capture routing for the four blocks, as wire bytes.
    pub capture: [[u8; 8]; 4],
    /// Output routing for the four blocks, as wire bytes.
    pub outputs: [[u8; 8]; 4],
    /// Raw 16-bit mixer gains, `[mix][channel]`.
    pub raw_mix: [[u16; 8]; 16],
    /// Macro mix region, kept as raw words.
    ///
    /// The official tool skips this and re-requests the mix state with `2AH` instead,
    /// so the internal layout of these words is not established by any reference
    /// implementation. Preserved verbatim so a round trip is lossless.
    pub macro_words: Box<[u32; 256]>,
    /// Global options.
    pub options: Options,
    /// Stereo link bitmap.
    pub links: Links,
    /// MIDI channel for CC control over USB.
    pub midi_usb: u8,
    /// MIDI channel for CC control over the DIN breakout.
    pub midi_din: u8,
    /// Output DC offsets for the eight 3.5mm outputs.
    pub dc_offsets: [i16; 8],
    /// EQ bands, `[block][channel][band]`.
    pub eq: Box<[[[EqBand; 4]; 8]; 2]>,
    /// Per-mix gain smoothing bitmap, bits 0-15.
    pub smoothing: u16,
    /// Trailing words the firmware does not currently use, preserved verbatim.
    pub reserved: Box<[u32; DUMP_WORDS - W_RESERVED]>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: SUPPORTED_VERSION,
            hpf: 0,
            capture: [[0; 8]; 4],
            outputs: [[0; 8]; 4],
            raw_mix: [[0; 8]; 16],
            macro_words: Box::new([0; 256]),
            options: Options::default(),
            links: Links::default(),
            midi_usb: 0,
            midi_din: 0,
            dc_offsets: [0; 8],
            eq: Box::new([[[EqBand::default(); 4]; 8]; 2]),
            smoothing: 0,
            reserved: Box::new([0; DUMP_WORDS - W_RESERVED]),
        }
    }
}

impl Config {
    /// The decoded source feeding a mixer or capture channel, if recognised.
    pub fn capture_source(&self, block: usize, channel: usize) -> Option<Source> {
        Source::from_wire(*self.capture.get(block)?.get(channel)?)
    }

    /// The decoded destination for an output channel, if recognised.
    pub fn output_destination(&self, block: usize, channel: usize) -> Option<Destination> {
        Destination::from_wire(*self.outputs.get(block)?.get(channel)?)
    }

    /// The eight sources feeding each mixer block, for CC map resolution.
    ///
    /// Unrecognised wire values fall back to bus 16, the module's conventional dead end.
    pub fn mixer_sources(&self) -> [[Source; 8]; 2] {
        core::array::from_fn(|b| {
            core::array::from_fn(|c| self.capture_source(b + 2, c).unwrap_or(Source::Bus(16)))
        })
    }

    /// Resolves the MIDI CC map for this configuration.
    pub fn cc_map(&self) -> crate::ccmap::CcMap {
        crate::ccmap::CcMap::resolve(self.links, self.options.mixer2, &self.mixer_sources())
    }
}
