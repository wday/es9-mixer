//! Decoding messages sent by the ES-9.

use crate::config::{
    Config, DUMP_WORDS, EqBand, Options, SUPPORTED_VERSION, W_CAPTURE, W_DC, W_EQ, W_HPF,
    W_LINKS_HI, W_LINKS_LO, W_MACRO, W_MIDI, W_OPTIONS, W_OUTPUTS, W_RAW_MIX, W_RESERVED,
    W_SMOOTHING, W_VERSION,
};
use crate::frame::{self, FrameError};
use crate::links::Links;

/// Command byte for a firmware 1.3 configuration dump.
pub const CMD_CONFIG_DUMP: u8 = 0x08;
/// Command byte for a firmware **1.2** configuration dump, which this crate rejects.
pub const CMD_CONFIG_DUMP_V12: u8 = 0x10;
/// Command byte for a mix state dump.
pub const CMD_MIX_DUMP: u8 = 0x11;
/// Command byte for a DSP usage report.
pub const CMD_USAGE: u8 = 0x12;
/// Command byte for a sample rate report.
pub const CMD_SAMPLE_RATE: u8 = 0x14;
/// Command byte for a text message.
pub const CMD_MESSAGE: u8 = 0x32;

/// One cell of the macro mix, as it appears in a `11H` dump.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MacroCell {
    /// The value addressed by this entry's MIDI CC number.
    ///
    /// Whether it means level or pan depends on the stereo configuration; resolve it
    /// with [`crate::ccmap::CcMap`].
    pub value: u8,
    /// The second byte of the entry.
    ///
    /// The config tool reads this as a pan value biased by 63, but writes pan through a
    /// different index entirely, so the two disagree and pan does not survive a reload
    /// in the reference tool. Preserved unmodified pending hardware verification.
    pub aux: u8,
}

/// A decoded mix state dump.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MixDump {
    /// Raw 16-bit gains, `[mix][channel]`.
    pub raw: [[u16; 8]; 16],
    /// Macro mix entries, indexed by MIDI CC number.
    pub macro_cells: Box<[MacroCell; 128]>,
}

/// DSP usage counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    /// Idle and loaded counters for each DSP. `dsp1` is `None` when inactive.
    pub dsp0: (u32, u32),
    /// Second DSP, present only when the module reports a non-zero counter.
    pub dsp1: Option<(u32, u32)>,
}

impl Usage {
    /// Load as a percentage, for one DSP's counter pair.
    pub fn percent(counters: (u32, u32)) -> f32 {
        let (idle, loaded) = counters;
        if idle >= 4096 {
            return 0.0;
        }
        (loaded as f32 - idle as f32) * 100.0 / (4096.0 - idle as f32)
    }
}

/// Anything the ES-9 can send that this crate understands.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// A full configuration dump.
    Config(Box<Config>),
    /// A full mix state dump.
    Mix(Box<MixDump>),
    /// DSP usage.
    Usage(Usage),
    /// Operating sample rate in hertz.
    SampleRate(u32),
    /// A text message: a version string, or feedback such as `"OK"` after a flash save.
    Message(String),
    /// A single raw mixer gain changed.
    RawMixChanged {
        /// Zero-based mix index.
        mix: u8,
        /// Zero-based channel index.
        channel: u8,
        /// New 16-bit gain.
        value: u16,
    },
    /// A recognised ES-9 message this crate does not decode.
    Unknown {
        /// The command byte.
        cmd: u8,
        /// Payload bytes.
        payload: Vec<u8>,
    },
}

/// Why a message could not be decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// The SysEx frame itself was malformed.
    Frame(FrameError),
    /// A payload was shorter than its command requires.
    ShortPayload {
        /// The command byte.
        cmd: u8,
        /// Bytes required.
        expected: usize,
        /// Bytes present.
        got: usize,
    },
    /// The module is running firmware 1.2, whose dump layout differs.
    ///
    /// The `version` field inside the payload is `3` in both 1.2 and 1.3, so it cannot
    /// be used to tell them apart; only the command byte distinguishes them.
    FirmwareTooOld,
    /// The dump's internal version field was not the supported one.
    UnsupportedVersion {
        /// Version reported by the module.
        found: u32,
    },
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Frame(e) => write!(f, "{e}"),
            Self::ShortPayload { cmd, expected, got } => write!(
                f,
                "command {cmd:#04X} needs {expected} payload bytes, got {got}"
            ),
            Self::FirmwareTooOld => write!(
                f,
                "module is running firmware 1.2; update to 1.3 or later (the dump format changed)"
            ),
            Self::UnsupportedVersion { found } => {
                write!(f, "unsupported config layout version {found}")
            }
        }
    }
}

impl core::error::Error for DecodeError {}

impl From<FrameError> for DecodeError {
    fn from(e: FrameError) -> Self {
        Self::Frame(e)
    }
}

/// Decodes one complete SysEx message from the ES-9.
pub fn decode(data: &[u8]) -> Result<Incoming, DecodeError> {
    let msg = frame::parse(data)?;
    match msg.cmd {
        CMD_CONFIG_DUMP => decode_config(msg.payload).map(|c| Incoming::Config(Box::new(c))),
        CMD_CONFIG_DUMP_V12 => Err(DecodeError::FirmwareTooOld),
        CMD_MIX_DUMP => decode_mix(msg.payload).map(|m| Incoming::Mix(Box::new(m))),
        CMD_USAGE => decode_usage(msg.payload),
        CMD_SAMPLE_RATE => decode_sample_rate(msg.payload),
        CMD_MESSAGE => Ok(Incoming::Message(decode_message(msg.payload))),
        c if c & 0xF0 == 0x60 => decode_raw_mix_change(c & 0x0F, msg.payload),
        c => Ok(Incoming::Unknown {
            cmd: c,
            payload: msg.payload.to_vec(),
        }),
    }
}

/// Reads a message string, dropping the trailing NUL the module appends.
fn decode_message(payload: &[u8]) -> String {
    let end = payload
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(payload.len());
    String::from_utf8_lossy(&payload[..end]).into_owned()
}

fn words(payload: &[u8], skip: usize) -> Vec<u32> {
    let (words, _) = payload[skip..].as_chunks::<3>();
    words.iter().map(|w| frame::join21(w)).collect()
}

/// Decodes a `08H` configuration dump payload, which begins with two zero bytes.
pub fn decode_config(payload: &[u8]) -> Result<Config, DecodeError> {
    if payload.len() < 2 {
        return Err(DecodeError::ShortPayload {
            cmd: CMD_CONFIG_DUMP,
            expected: 2,
            got: payload.len(),
        });
    }
    let w = words(payload, 2);
    if w.len() < DUMP_WORDS {
        return Err(DecodeError::ShortPayload {
            cmd: CMD_CONFIG_DUMP,
            expected: 2 + DUMP_WORDS * 3,
            got: payload.len(),
        });
    }
    let version = w[W_VERSION];
    if version != SUPPORTED_VERSION {
        return Err(DecodeError::UnsupportedVersion { found: version });
    }

    let mut c = Config {
        version,
        hpf: w[W_HPF],
        options: Options::from_bits(w[W_OPTIONS]),
        links: Links((w[W_LINKS_HI] << 16) | w[W_LINKS_LO]),
        midi_usb: ((w[W_MIDI] >> 8) & 0xFF) as u8,
        midi_din: (w[W_MIDI] & 0xFF) as u8,
        smoothing: (w[W_SMOOTHING] & 0xFFFF) as u16,
        ..Config::default()
    };

    for block in 0..4 {
        for ch in 0..8 {
            c.capture[block][ch] = w[W_CAPTURE + block * 8 + ch] as u8;
            c.outputs[block][ch] = w[W_OUTPUTS + block * 8 + ch] as u8;
        }
    }
    for mix in 0..16 {
        for ch in 0..8 {
            c.raw_mix[mix][ch] = w[W_RAW_MIX + mix * 8 + ch] as u16;
        }
    }
    c.macro_words.copy_from_slice(&w[W_MACRO..W_MACRO + 256]);
    for i in 0..8 {
        c.dc_offsets[i] = frame::sign_extend16(w[W_DC + i]) as i16;
    }
    let mut p = W_EQ;
    for block in 0..2 {
        for ch in 0..8 {
            for band in 0..4 {
                let kind = w[p];
                c.eq[block][ch][band] = EqBand {
                    enabled: kind & 1 != 0,
                    kind: (kind >> 1) as u8,
                    freq: w[p + 1],
                    q: w[p + 2],
                    gain: frame::sign_extend16(w[p + 3]),
                };
                p += 4;
            }
        }
    }
    c.reserved.copy_from_slice(&w[W_RESERVED..DUMP_WORDS]);
    Ok(c)
}

/// Decodes an `11H` mix dump payload.
pub fn decode_mix(payload: &[u8]) -> Result<MixDump, DecodeError> {
    const RAW_BYTES: usize = 128 * 3;
    const NEEDED: usize = RAW_BYTES + 256;
    if payload.len() < NEEDED {
        return Err(DecodeError::ShortPayload {
            cmd: CMD_MIX_DUMP,
            expected: NEEDED,
            got: payload.len(),
        });
    }
    let mut raw = [[0u16; 8]; 16];
    for (i, chunk) in payload[..RAW_BYTES].as_chunks::<3>().0.iter().enumerate() {
        raw[i / 8][i % 8] = frame::join21(chunk) as u16;
    }
    let mut cells = Box::new([MacroCell::default(); 128]);
    for (i, pair) in payload[RAW_BYTES..NEEDED]
        .as_chunks::<2>()
        .0
        .iter()
        .enumerate()
    {
        cells[i] = MacroCell {
            value: pair[0],
            aux: pair[1],
        };
    }
    Ok(MixDump {
        raw,
        macro_cells: cells,
    })
}

fn decode_usage(payload: &[u8]) -> Result<Incoming, DecodeError> {
    if payload.len() < 8 {
        return Err(DecodeError::ShortPayload {
            cmd: CMD_USAGE,
            expected: 8,
            got: payload.len(),
        });
    }
    let u: Vec<u32> = payload[..8]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| frame::join14(p))
        .collect();
    Ok(Incoming::Usage(Usage {
        dsp0: (u[0], u[1]),
        dsp1: (u[2] != 0).then_some((u[2], u[3])),
    }))
}

fn decode_sample_rate(payload: &[u8]) -> Result<Incoming, DecodeError> {
    if payload.len() < 3 {
        return Err(DecodeError::ShortPayload {
            cmd: CMD_SAMPLE_RATE,
            expected: 3,
            got: payload.len(),
        });
    }
    Ok(Incoming::SampleRate(frame::join21(&payload[..3]) * 4))
}

fn decode_raw_mix_change(mix: u8, payload: &[u8]) -> Result<Incoming, DecodeError> {
    if payload.len() < 4 {
        return Err(DecodeError::ShortPayload {
            cmd: 0x60 | mix,
            expected: 4,
            got: payload.len(),
        });
    }
    Ok(Incoming::RawMixChanged {
        mix,
        channel: payload[0],
        value: frame::join21(&payload[1..4]) as u16,
    })
}
