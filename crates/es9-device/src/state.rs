//! The device model.
//!
//! State is held here and rendered from; it is never read back out of a UI widget. Every
//! change goes through [`crate::action`], so the model, the outgoing MIDI stream, and the
//! undo history cannot drift apart.

use es9_protocol::ccmap::CcMap;
use es9_protocol::config::Config;
use es9_protocol::decode::{Incoming, MacroCell, Usage};

/// Which flash slot the user is currently editing.
///
/// This is bookkeeping, not something the module reports. Connecting over USB always puts
/// the module into hosted mode, so editing the standalone configuration means restoring
/// it first — see [`crate::action::Session::begin_edit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum EditTarget {
    /// The configuration used when a USB host is present.
    #[default]
    Hosted,
    /// The configuration used when the module powers up with no USB connection.
    Standalone,
}

impl EditTarget {
    /// The flash slot this target maps to.
    pub const fn slot(self) -> es9_protocol::encode::FlashSlot {
        match self {
            Self::Hosted => es9_protocol::encode::FlashSlot::Hosted,
            Self::Standalone => es9_protocol::encode::FlashSlot::Standalone,
        }
    }
}

/// What the module has told us about itself.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Identity {
    /// Firmware version string, from a `22H` request.
    pub version: Option<String>,
    /// Operating sample rate in hertz.
    pub sample_rate: Option<u32>,
    /// Most recent DSP usage report.
    pub usage: Option<Usage>,
    /// The most recent text message, such as `"OK"` after a flash save.
    pub last_message: Option<String>,
}

/// Tracks the module's echo of the host's own macro writes.
///
/// The module answers every `34H` macro write with a **full** `11H` mix dump. Holding a
/// fader therefore produces a stream of dumps trailing the pointer, each carrying the
/// value of an earlier step, and folding them in verbatim walks the fader backwards under
/// the hand holding it. A written byte is pinned to what the host wrote until the module
/// confirms it by echoing that exact value back.
///
/// The pin is bounded by `in_flight`, and it has to be. Anything else that moves a macro
/// draws a dump too — a CC from a controller, which in this rig is continuous — so a pin
/// that could only be released by a matching echo would freeze a fader for good the first
/// time a write went missing. Once no write is outstanding the module is the authority
/// again, whatever moved it.
#[derive(Debug, Clone, PartialEq)]
pub struct EchoGuard {
    value: Box<[Option<u8>; 128]>,
    aux: Box<[Option<u8>; 128]>,
    in_flight: u32,
}

impl Default for EchoGuard {
    fn default() -> Self {
        Self {
            value: Box::new([None; 128]),
            aux: Box::new([None; 128]),
            in_flight: 0,
        }
    }
}

impl EchoGuard {
    /// Writes still unaccounted for by an inbound dump.
    ///
    /// Exposed so a test can assert that the guard *releases*, rather than inferring it
    /// from values that would look the same either way.
    pub fn in_flight(&self) -> u32 {
        self.in_flight
    }

    /// Records a host write to a macro value cell, and the dump it will draw.
    pub(crate) fn wrote_value(&mut self, cc: u8, value: u8) {
        self.value[usize::from(cc)] = Some(value);
        self.in_flight = self.in_flight.saturating_add(1);
    }

    /// Records a host write to a macro aux cell — where pan is stored — and its dump.
    pub(crate) fn wrote_aux(&mut self, cc: u8, stored: u8) {
        self.aux[usize::from(cc)] = Some(stored);
        self.in_flight = self.in_flight.saturating_add(1);
    }

    /// Accounts for one inbound dump. Returns whether writes are still outstanding, in
    /// which case this dump may be trailing them.
    pub(crate) fn dump(&mut self) -> bool {
        self.in_flight = self.in_flight.saturating_sub(1);
        self.in_flight > 0
    }

    /// Drops every pin. A full configuration read is an explicit resynchronisation, and
    /// holding local values across one would be pinning against the thing that resolves
    /// the disagreement.
    pub(crate) fn reset(&mut self) {
        self.value.fill(None);
        self.aux.fill(None);
        self.in_flight = 0;
    }

    pub(crate) fn settle_value(&mut self, i: usize, held: &mut u8, incoming: u8, trailing: bool) {
        settle(&mut self.value[i], held, incoming, trailing);
    }

    pub(crate) fn settle_aux(&mut self, i: usize, held: &mut u8, incoming: u8, trailing: bool) {
        settle(&mut self.aux[i], held, incoming, trailing);
    }
}

/// Folds one byte of an inbound dump into the model, honouring a pending host write.
///
/// The echo of the host's own write is its acknowledgement: seeing that exact value come
/// back releases the pin. A dump that disagrees is held off only while writes are still
/// outstanding, because that is the only situation in which it can be stale.
fn settle(expect: &mut Option<u8>, held: &mut u8, incoming: u8, trailing: bool) {
    if trailing && matches!(*expect, Some(e) if e != incoming) {
        return;
    }
    *expect = None;
    *held = incoming;
}

/// Everything known about the connected module.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceState {
    /// The current configuration.
    pub config: Config,
    /// Macro mix values, indexed by MIDI CC number.
    pub macro_values: Box<[u8; 128]>,
    /// The second byte of each macro entry: **this is where pan is stored**.
    ///
    /// Pan is written to CC `(m+1)*8 + ch` but the module keeps it here, in the aux byte
    /// of the level cell, as `written - 1`. See `docs/es9-sysex-protocol.md` §11.
    pub macro_aux: Box<[u8; 128]>,
    /// What the module has reported about itself.
    pub identity: Identity,
    /// Which flash slot the user is editing.
    pub editing: EditTarget,
    /// Whether the configuration has changed since it was last loaded or saved.
    pub dirty: bool,
    /// Bookkeeping for the module's echo of the host's own writes.
    ///
    /// Maintained by [`DeviceState::apply`] and [`DeviceState::ingest`] together, which
    /// is the only pair that knows both what was written and what came back. Nothing
    /// else should touch it.
    pub echo: EchoGuard,
}

impl Default for DeviceState {
    fn default() -> Self {
        Self {
            config: Config::default(),
            macro_values: Box::new([0; 128]),
            macro_aux: Box::new([0; 128]),
            identity: Identity::default(),
            editing: EditTarget::default(),
            dirty: false,
            echo: EchoGuard::default(),
        }
    }
}

impl DeviceState {
    /// The resolved MIDI CC map for the current configuration.
    pub fn cc_map(&self) -> CcMap {
        self.config.cc_map()
    }

    /// Folds an inbound message into the state.
    ///
    /// A full dump replaces the configuration and clears the dirty flag, since what is
    /// held now matches what the module holds.
    pub fn ingest(&mut self, msg: Incoming) {
        match msg {
            Incoming::Config(c) => {
                self.config = *c;
                self.dirty = false;
                self.echo.reset();
            }
            Incoming::Mix(m) => {
                // One dump per macro write, so this accounts for at most one of them.
                let trailing = self.echo.dump();
                // The raw matrix is not pinned. It is read-only in the UI, and the host
                // never writes the raw value a macro write produces — the module derives
                // it — so there is no expected value to acknowledge. A trailing dump
                // therefore walks the raw readout backwards for as long as a drag lasts.
                for mix in 0..16 {
                    for ch in 0..8 {
                        self.config.raw_mix[mix][ch] = m.raw[mix][ch];
                    }
                }
                for (i, cell) in m.macro_cells.iter().enumerate() {
                    let MacroCell { value, aux } = *cell;
                    self.echo
                        .settle_value(i, &mut self.macro_values[i], value, trailing);
                    self.echo
                        .settle_aux(i, &mut self.macro_aux[i], aux, trailing);
                }
            }
            Incoming::Usage(u) => self.identity.usage = Some(u),
            Incoming::SampleRate(r) => self.identity.sample_rate = Some(r),
            Incoming::Message(s) => {
                // The module answers a version request and an operation acknowledgement
                // with the same message type, so the first string we see is the version
                // and later ones are feedback.
                if self.identity.version.is_none() {
                    self.identity.version = Some(s.clone());
                }
                self.identity.last_message = Some(s);
            }
            Incoming::RawMixChanged {
                mix,
                channel,
                value,
            } => {
                if let Some(cell) = self
                    .config
                    .raw_mix
                    .get_mut(usize::from(mix))
                    .and_then(|row| row.get_mut(usize::from(channel)))
                {
                    *cell = value;
                }
            }
            Incoming::Unknown { .. } => {}
        }
    }
}
