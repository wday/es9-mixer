//! The protocol monitor.
//!
//! Every byte in either direction passes through here, which is what makes the monitor
//! trustworthy: it cannot miss traffic that took a shortcut. It also serves a practical
//! purpose the manual itself recommends — the byte after `34H` is the MIDI CC number for
//! the control you just moved, so watching the stream is how you identify a control's CC.
//!
//! Timestamps are supplied by the caller rather than read from a clock, so this works
//! unchanged in a browser, where `Instant::now` is not available.

use std::collections::VecDeque;

/// Which way a message travelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum Direction {
    /// Host to module.
    Tx,
    /// Module to host.
    Rx,
}

/// One logged message.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Monotonic sequence number, so entries stay ordered without a clock.
    pub seq: u64,
    /// Direction of travel.
    pub direction: Direction,
    /// The raw bytes.
    pub bytes: Vec<u8>,
    /// Milliseconds since the log was created, as supplied by the caller.
    pub at_ms: u64,
}

impl Entry {
    /// The message rendered as space-separated uppercase hex.
    pub fn hex(&self) -> String {
        self.bytes
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// A one-line description of what the message means.
    pub fn describe(&self) -> String {
        match self.direction {
            Direction::Rx => match es9_protocol::decode(&self.bytes) {
                Ok(msg) => describe_incoming(&msg),
                Err(e) => format!("undecodable: {e}"),
            },
            Direction::Tx => describe_outgoing(&self.bytes),
        }
    }
}

fn describe_incoming(msg: &es9_protocol::Incoming) -> String {
    use es9_protocol::Incoming as I;
    match msg {
        I::Config(_) => "configuration dump".to_string(),
        I::Mix(_) => "mix state dump".to_string(),
        I::Usage(_) => "DSP usage".to_string(),
        I::SampleRate(r) => format!("sample rate {r} Hz"),
        I::Message(s) => format!("message {s:?}"),
        I::RawMixChanged {
            mix,
            channel,
            value,
        } => format!("raw mix {}/{} = {value}", mix + 1, channel + 1),
        I::Unknown { cmd, .. } => format!("unrecognised command {cmd:#04X}"),
    }
}

fn describe_outgoing(bytes: &[u8]) -> String {
    let Ok(msg) = es9_protocol::frame::parse(bytes) else {
        return "malformed".to_string();
    };
    let p = msg.payload;
    match msg.cmd {
        0x02 => "display message".to_string(),
        0x09 => format!("config upload chunk {}", p.first().copied().unwrap_or(0)),
        0x22 => "request version".to_string(),
        0x23 => "request configuration".to_string(),
        0x24 => format!("save to flash, {}", slot_name(p.first().copied())),
        0x25 => format!("restore from flash, {}", slot_name(p.first().copied())),
        0x26 => "reset to defaults".to_string(),
        0x2A => "request mix".to_string(),
        0x2B => "request usage".to_string(),
        0x2C => "request sample rate".to_string(),
        // Named pairs rather than a bare bitmap: the point of the monitor is that you
        // can see which filters a write actually engages, and the pairing is not obvious.
        0x31 => match p.first() {
            Some(&bits) => {
                let on: Vec<String> = (0..es9_protocol::dcblock::PAIR_COUNT as u8)
                    .filter(|&b| es9_protocol::dcblock::is_on(bits, b))
                    .filter_map(es9_protocol::dcblock::label)
                    .collect();
                if on.is_empty() {
                    "DC blocking: all filters off".to_string()
                } else {
                    format!("DC blocking: {}", on.join(", "))
                }
            }
            None => "set DC blocking".to_string(),
        },
        0x32 => "set options".to_string(),
        0x33 => "set stereo link".to_string(),
        // The manual's own advice for identifying a control's CC number.
        0x34 => match p {
            [cc, value, ..] => format!("macro mix: CC {cc} = {value}"),
            _ => "macro mix".to_string(),
        },
        0x35 => "set MIDI channels".to_string(),
        // Named with its channel and value: an offset that is not taking effect is a
        // routing problem, and the first thing to check is that the write went where you
        // think it did.
        0x36 => match p {
            [ch, rest @ ..] if rest.len() >= 3 => {
                let raw = es9_protocol::frame::join21(rest);
                let value = es9_protocol::frame::sign_extend16(raw);
                format!("set DC offset: output {} = {value}", ch + 1)
            }
            [ch, ..] => format!("set DC offset: output {}", ch + 1),
            _ => "set DC offset".to_string(),
        },
        0x39 => "set EQ band".to_string(),
        0x3A => "set smoothing".to_string(),
        c @ 0x40..=0x43 => format!("set capture routing, block {}", c - 0x40),
        c @ 0x50..=0x53 => format!("set output routing, block {}", c - 0x50),
        c if c & 0xF0 == 0x60 => format!("set raw mix, mix {}", (c & 0x0F) + 1),
        c => format!("command {c:#04X}"),
    }
}

fn slot_name(slot: Option<u8>) -> &'static str {
    match slot {
        Some(0) => "standalone",
        Some(1) => "hosted",
        _ => "unknown slot",
    }
}

/// A bounded log of recent traffic.
#[derive(Debug)]
pub struct Monitor {
    entries: VecDeque<Entry>,
    capacity: usize,
    next_seq: u64,
}

impl Monitor {
    /// Creates a monitor holding at most `capacity` entries.
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(capacity.min(1024)),
            capacity,
            next_seq: 0,
        }
    }

    /// Records a message, discarding the oldest if the log is full.
    pub fn record(&mut self, direction: Direction, bytes: &[u8], at_ms: u64) {
        if self.entries.len() == self.capacity {
            self.entries.pop_front();
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.entries.push_back(Entry {
            seq,
            direction,
            bytes: bytes.to_vec(),
            at_ms,
        });
    }

    /// Entries from oldest to newest.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter()
    }

    /// Number of entries currently held.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the log is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Empties the log.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl Default for Monitor {
    fn default() -> Self {
        Self::new(500)
    }
}
