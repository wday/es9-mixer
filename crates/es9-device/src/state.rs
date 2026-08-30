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
            }
            Incoming::Mix(m) => {
                for mix in 0..16 {
                    for ch in 0..8 {
                        self.config.raw_mix[mix][ch] = m.raw[mix][ch];
                    }
                }
                for (i, cell) in m.macro_cells.iter().enumerate() {
                    let MacroCell { value, aux } = *cell;
                    self.macro_values[i] = value;
                    self.macro_aux[i] = aux;
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
