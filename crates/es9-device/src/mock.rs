//! An offline ES-9.
//!
//! Speaks the same protocol as the hardware, so the whole application can be built and
//! exercised with nothing plugged in. Where the real module's behaviour is unverified,
//! the mock makes it configurable rather than picking an answer and pretending — see
//! [`MockEs9::echo_writes`].

use es9_protocol::config::{Config, EqBand, Options};
use es9_protocol::decode::{self, MacroCell};
use es9_protocol::encode;
use es9_protocol::frame::{self, push21};

/// A simulated ES-9.
#[derive(Debug, Clone)]
pub struct MockEs9 {
    /// The module's live configuration.
    pub config: Config,
    /// Macro mix values indexed by CC number.
    pub macro_values: Box<[u8; 128]>,
    /// Second byte of each macro entry.
    pub macro_aux: Box<[u8; 128]>,
    /// Flash slots: index 0 standalone, index 1 hosted.
    pub flash: [Box<Config>; 2],
    /// Version string reported for a `22H` request.
    pub version: String,
    /// Reported sample rate in hertz.
    pub sample_rate: u32,
    /// Whether the module reports state back after the host's own writes.
    ///
    /// **Measured on hardware 2026-08-30 (Q6): it does.** A `34H` macro write draws an
    /// unsolicited full `11H` mix dump carrying the written value, and a raw write is
    /// echoed as `60H+mix`. A client that assumes silence will loop, so this now defaults
    /// to **on** and the quiet case is the one that has to be asked for.
    pub echo_writes: bool,
    upload: UploadBuffer,
}

#[derive(Debug, Clone)]
struct UploadBuffer {
    words: Vec<u32>,
    received: [bool; encode::UPLOAD_CHUNKS],
}

impl Default for UploadBuffer {
    fn default() -> Self {
        Self {
            words: vec![0; es9_protocol::config::DUMP_WORDS],
            received: [false; encode::UPLOAD_CHUNKS],
        }
    }
}

impl Default for MockEs9 {
    fn default() -> Self {
        let config = Config::default();
        Self {
            flash: [Box::new(config.clone()), Box::new(config.clone())],
            config,
            macro_values: Box::new([0; 128]),
            macro_aux: Box::new([0; 128]),
            version: "ES-9 1.3.1".to_string(),
            sample_rate: 48_000,
            echo_writes: true,
            upload: UploadBuffer::default(),
        }
    }
}

impl MockEs9 {
    /// Handles one inbound message, returning whatever the module would send back.
    ///
    /// Unrecognised or malformed input is ignored, as the hardware ignores traffic that
    /// does not match its header.
    pub fn receive(&mut self, data: &[u8]) -> Vec<Vec<u8>> {
        let Ok(msg) = frame::parse(data) else {
            return Vec::new();
        };
        let p = msg.payload;
        match msg.cmd {
            0x22 => vec![self.message(&self.version.clone())],
            0x23 => vec![encode::config_dump(&self.config)],
            0x2A => vec![self.mix_dump()],
            0x2B => vec![self.usage()],
            0x2C => vec![self.sample_rate_msg()],
            0x24 => {
                if let Some(&slot) = p.first()
                    && let Some(dst) = self.flash.get_mut(usize::from(slot))
                {
                    **dst = self.config.clone();
                    return vec![self.message("OK")];
                }
                Vec::new()
            }
            0x25 => {
                if let Some(&slot) = p.first()
                    && let Some(src) = self.flash.get(usize::from(slot))
                {
                    self.config = (**src).clone();
                }
                Vec::new()
            }
            0x26 => {
                self.config = Config::default();
                Vec::new()
            }
            0x09 => {
                self.absorb_upload(p);
                Vec::new()
            }
            0x31 => {
                if let Some(&bits) = p.first() {
                    self.config.hpf = u32::from(bits);
                }
                Vec::new()
            }
            0x32 => {
                if let Some(&bits) = p.first() {
                    self.config.options = Options::from_bits(u32::from(bits));
                }
                Vec::new()
            }
            0x33 => {
                if let [index, state, ..] = p {
                    let bit = 1u32 << index;
                    if *state != 0 {
                        self.config.links.0 |= bit;
                    } else {
                        self.config.links.0 &= !bit;
                    }
                }
                Vec::new()
            }
            0x34 => {
                if let [cc, value, ..] = p {
                    self.macro_values[usize::from(*cc)] = *value;
                    // A write to a CC that is a *pan* for the current link configuration
                    // does not stay where it was written: the module stores it in the
                    // aux byte of the corresponding level cell, one less than the value
                    // sent (checklist H3). Reproduced here so the offline build pans the
                    // way the hardware does rather than the way the address suggests.
                    let map = self.config.cc_map();
                    if let Some(level_cc) = map.level_cc_for_pan_cc(*cc) {
                        self.macro_aux[usize::from(level_cc)] =
                            es9_protocol::scales::written_to_stored(*value);
                    }
                    // Macro drives raw (S1), so the cell the CC addresses follows it.
                    let (mix, channel) = (usize::from(*cc / 8), usize::from(*cc % 8));
                    if let Some(slot) = self
                        .config
                        .raw_mix
                        .get_mut(mix)
                        .and_then(|row| row.get_mut(channel))
                    {
                        *slot = es9_protocol::scales::macro_to_raw(*value);
                    }
                    // The hardware answers a macro write with a whole mix dump, not an
                    // echo of the write. That is a lot of traffic per fader step, which
                    // is what makes send coalescing a requirement rather than a polish.
                    if self.echo_writes {
                        return vec![self.mix_dump()];
                    }
                }
                Vec::new()
            }
            0x35 => {
                if let [usb, din, ..] = p {
                    self.config.midi_usb = *usb;
                    self.config.midi_din = *din;
                }
                Vec::new()
            }
            0x36 => {
                if let [channel, rest @ ..] = p
                    && rest.len() >= 3
                    && let Some(slot) = self.config.dc_offsets.get_mut(usize::from(*channel))
                {
                    *slot = frame::sign_extend16(frame::join21(rest)) as i16;
                }
                Vec::new()
            }
            0x39 => {
                self.set_filter(p);
                Vec::new()
            }
            0x3A => {
                if let [mix, state, ..] = p {
                    let bit = 1u16 << mix;
                    if *state != 0 {
                        self.config.smoothing |= bit;
                    } else {
                        self.config.smoothing &= !bit;
                    }
                }
                Vec::new()
            }
            c @ 0x40..=0x43 => {
                if p.len() >= 8 {
                    self.config.capture[usize::from(c - 0x40)].copy_from_slice(&p[..8]);
                }
                Vec::new()
            }
            c @ 0x50..=0x53 => {
                if p.len() >= 8 {
                    self.config.outputs[usize::from(c - 0x50)].copy_from_slice(&p[..8]);
                }
                Vec::new()
            }
            c if c & 0xF0 == 0x60 => {
                let mix = c & 0x0F;
                if let [channel, rest @ ..] = p
                    && rest.len() >= 3
                {
                    let value = frame::join21(rest) as u16;
                    if let Some(slot) = self
                        .config
                        .raw_mix
                        .get_mut(usize::from(mix))
                        .and_then(|row| row.get_mut(usize::from(*channel)))
                    {
                        *slot = value;
                    }
                    if self.echo_writes {
                        return vec![encode::set_raw_mix(mix, *channel, value)];
                    }
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn set_filter(&mut self, p: &[u8]) {
        let [addr, band, kind, rest @ ..] = p else {
            return;
        };
        if rest.len() < 9 {
            return;
        }
        let (block, channel) = (usize::from(addr / 8), usize::from(addr % 8));
        let Some(slot) = self
            .config
            .eq
            .get_mut(block)
            .and_then(|b| b.get_mut(channel))
            .and_then(|c| c.get_mut(usize::from(*band)))
        else {
            return;
        };
        *slot = EqBand {
            enabled: kind & 1 != 0,
            kind: kind >> 1,
            freq: frame::join21(&rest[0..3]),
            q: frame::join21(&rest[3..6]),
            gain: frame::sign_extend16(frame::join21(&rest[6..9])),
        };
    }

    fn absorb_upload(&mut self, p: &[u8]) {
        let [index, _zero, rest @ ..] = p else {
            return;
        };
        let i = usize::from(*index);
        if i >= encode::UPLOAD_CHUNKS {
            return;
        }
        let start = i * encode::UPLOAD_WORDS_PER_CHUNK;
        for (n, chunk) in rest
            .chunks_exact(3)
            .take(encode::UPLOAD_WORDS_PER_CHUNK)
            .enumerate()
        {
            self.upload.words[start + n] = frame::join21(chunk);
        }
        self.upload.received[i] = true;

        if self.upload.received.iter().all(|&r| r) {
            let mut payload = vec![0u8, 0u8];
            for w in &self.upload.words {
                payload.extend_from_slice(&frame::split21(*w));
            }
            if let Ok(c) = decode::decode_config(&payload) {
                self.config = c;
            }
            self.upload = UploadBuffer::default();
        }
    }

    fn message(&self, text: &str) -> Vec<u8> {
        let mut v = frame::begin(0x32);
        v.extend(text.bytes().filter(|b| b & 0x80 == 0));
        v.push(0); // the module NUL-terminates its strings
        frame::end(v)
    }

    fn mix_dump(&self) -> Vec<u8> {
        let mut v = frame::begin(0x11);
        for mix in 0..16 {
            for ch in 0..8 {
                push21(&mut v, u32::from(self.config.raw_mix[mix][ch]));
            }
        }
        for i in 0..128 {
            v.push(self.macro_values[i] & 0x7F);
            v.push(self.macro_aux[i] & 0x7F);
        }
        frame::end(v)
    }

    fn usage(&self) -> Vec<u8> {
        let mut v = frame::begin(0x12);
        for value in [200u32, 900, 0, 0] {
            v.push(((value >> 7) & 0x7F) as u8);
            v.push((value & 0x7F) as u8);
        }
        frame::end(v)
    }

    fn sample_rate_msg(&self) -> Vec<u8> {
        let mut v = frame::begin(0x14);
        push21(&mut v, self.sample_rate / 4);
        frame::end(v)
    }

    /// Convenience for tests: the macro cells as a mix dump would report them.
    pub fn macro_cells(&self) -> Vec<MacroCell> {
        (0..128)
            .map(|i| MacroCell {
                value: self.macro_values[i],
                aux: self.macro_aux[i],
            })
            .collect()
    }
}
