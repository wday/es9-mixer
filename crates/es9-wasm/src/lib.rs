//! Browser bridge.
//!
//! Exposes the device model to JavaScript, driving the offline mock. The method surface
//! is deliberately the same shape the native backend will present, so the frontend does
//! not change when the mock is swapped for a real module over MIDI.

#![forbid(unsafe_code)]

use es9_device::action::Action;
use es9_device::monitor::{Direction, Monitor};
use es9_device::state::EditTarget;
use es9_device::{DeviceState, MockEs9, Session, mixer_defaults, view};
use es9_protocol::ccmap::Control;
use es9_protocol::config::Options;
use es9_protocol::tables::Family;
use serde::Serialize;
use wasm_bindgen::prelude::*;

/// The complete UI state in one object, so the frontend renders from a single snapshot.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    status: view::Status,
    mixes: Vec<view::Mix>,
    routing: view::Routing,
    dc_filters: Vec<view::DcFilter>,
    dc_offsets: Vec<view::DcOffset>,
    cc_rows: Vec<view::CcRow>,
    links: Vec<LinkRow>,
    sources: Vec<Choice>,
    destinations: Vec<Choice>,
    can_undo: bool,
    can_redo: bool,
}

/// One selectable routing option.
#[derive(Serialize)]
struct Choice {
    wire: u8,
    label: String,
}

/// One stereo link toggle.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LinkRow {
    family: u8,
    even_index: u8,
    label: String,
    linked: bool,
}

/// A logged message, rendered for display.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LogRow {
    seq: u64,
    direction: Direction,
    at_ms: u64,
    hex: String,
    description: String,
}

/// A CC whose meaning changed, for the warning a link toggle raises.
#[derive(Serialize)]
struct CcChangeRow {
    cc: u8,
    before: Option<String>,
    after: Option<String>,
}

/// Families in the order the frontend refers to them by index.
const FAMILIES: [Family; 6] = [
    Family::Input,
    Family::Bus,
    Family::UsbLow,
    Family::UsbHigh,
    Family::Mix,
    Family::SpdifOrMix2,
];

fn family_index(family: Family) -> u8 {
    FAMILIES.iter().position(|f| *f == family).unwrap_or(0) as u8
}

/// The device model plus a simulated module.
#[wasm_bindgen]
pub struct Bridge {
    session: Session,
    mock: MockEs9,
    monitor: Monitor,
    clock_ms: u64,
}

impl Default for Bridge {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl Bridge {
    /// Creates a bridge with a module already holding the standalone defaults.
    ///
    /// Starting from a real patch rather than an empty one means the interface shows
    /// something meaningful immediately, and matches the rig this is being built for.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        let mut mock = MockEs9::default();
        mock.config = mixer_defaults::standalone();
        *mock.flash[0] = mixer_defaults::standalone();
        *mock.flash[1] = mixer_defaults::hosted();
        // A sensible opening mix so the faders are not all at silence.
        for cc in 0..16u8 {
            mock.macro_values[usize::from(cc)] = es9_protocol::scales::MACRO_UNITY;
        }

        let mut bridge = Self {
            session: Session::default(),
            mock,
            monitor: Monitor::new(500),
            clock_ms: 0,
        };
        let messages = Session::request_all();
        bridge.pump(messages);
        bridge
    }

    /// Sends messages to the module and folds its replies back into the model.
    fn pump(&mut self, messages: Vec<Vec<u8>>) {
        for message in messages {
            self.clock_ms += 1;
            self.monitor.record(Direction::Tx, &message, self.clock_ms);
            for reply in self.mock.receive(&message) {
                self.clock_ms += 1;
                self.monitor.record(Direction::Rx, &reply, self.clock_ms);
                if let Ok(incoming) = es9_protocol::decode(&reply) {
                    self.session.state.ingest(incoming);
                }
            }
        }
    }

    fn dispatch(&mut self, action: Action) -> JsValue {
        let applied = self.session.dispatch(action);
        self.pump(applied.messages);
        let map = self.session.state.cc_map();
        let rows: Vec<CcChangeRow> = applied
            .cc_changes
            .iter()
            .map(|c| CcChangeRow {
                cc: c.cc,
                before: c.before.map(|_| describe(&map, c.cc, c.before)),
                after: c.after.map(|_| describe(&map, c.cc, c.after)),
            })
            .collect();
        to_js(&rows)
    }

    /// The full UI state.
    #[wasm_bindgen(js_name = snapshot)]
    pub fn snapshot(&self) -> JsValue {
        let state: &DeviceState = &self.session.state;
        let snapshot = Snapshot {
            status: view::status(state),
            mixes: view::mixer(state),
            routing: view::routing(state),
            dc_filters: view::dc_filters(state),
            dc_offsets: view::dc_offsets(state),
            cc_rows: view::cc_rows(state),
            links: view::links(state)
                .into_iter()
                .map(|(family, even_index, label, linked)| LinkRow {
                    family: family_index(family),
                    even_index,
                    label,
                    linked,
                })
                .collect(),
            sources: view::available_sources()
                .into_iter()
                .map(|(wire, label)| Choice { wire, label })
                .collect(),
            destinations: view::available_destinations()
                .into_iter()
                .map(|(wire, label)| Choice { wire, label })
                .collect(),
            can_undo: self.session.can_undo(),
            can_redo: self.session.can_redo(),
        };
        to_js(&snapshot)
    }

    /// Sets a macro mix control by its MIDI CC number.
    #[wasm_bindgen(js_name = setMacro)]
    pub fn set_macro(&mut self, cc: u8, value: u8) -> JsValue {
        self.dispatch(Action::SetMacro { cc, value })
    }

    /// Sets one raw mixer gain.
    #[wasm_bindgen(js_name = setRawMix)]
    pub fn set_raw_mix(&mut self, mix: u8, channel: u8, value: u16) -> JsValue {
        self.dispatch(Action::SetRawMix {
            mix,
            channel,
            value,
        })
    }

    /// Points a capture channel at a source.
    #[wasm_bindgen(js_name = setCapture)]
    pub fn set_capture(&mut self, block: u8, channel: u8, wire: u8) -> JsValue {
        self.dispatch(Action::SetCapture {
            block,
            channel,
            source: wire,
        })
    }

    /// Points an output channel at a destination.
    #[wasm_bindgen(js_name = setOutput)]
    pub fn set_output(&mut self, block: u8, channel: u8, wire: u8) -> JsValue {
        self.dispatch(Action::SetOutput {
            block,
            channel,
            destination: wire,
        })
    }

    /// Toggles a stereo link. The returned rows describe every CC this rewrote.
    #[wasm_bindgen(js_name = setLink)]
    pub fn set_link(&mut self, family: u8, even_index: u8, on: bool) -> JsValue {
        let family = FAMILIES
            .get(usize::from(family))
            .copied()
            .unwrap_or(Family::Input);
        self.dispatch(Action::SetLink {
            family,
            even_index,
            on,
        })
    }

    /// Sets the global options.
    #[wasm_bindgen(js_name = setOptions)]
    pub fn set_options(&mut self, mixer2: bool, midi_thru: bool) -> JsValue {
        self.dispatch(Action::SetOptions(Options { mixer2, midi_thru }))
    }

    /// Enables or disables gain smoothing for one mix.
    #[wasm_bindgen(js_name = setSmoothing)]
    pub fn set_smoothing(&mut self, mix: u8, on: bool) -> JsValue {
        self.dispatch(Action::SetSmoothing { mix, on })
    }

    /// Turns the DC-blocking filter for one input pair on or off.
    ///
    /// `bit` addresses a pair, not an input: the filters switch in pairs.
    #[wasm_bindgen(js_name = setDcBlock)]
    pub fn set_dc_block(&mut self, bit: u8, on: bool) -> JsValue {
        self.dispatch(Action::SetDcBlock { bit, on })
    }

    /// The module's current configuration, as the raw `08H` dump bytes.
    ///
    /// The same bytes the desktop build writes to a preset file, so both builds store
    /// presets in the module's own format rather than one of ours.
    #[wasm_bindgen(js_name = configDump)]
    pub fn config_dump(&self) -> Vec<u8> {
        es9_protocol::encode::config_dump(&self.session.state.config)
    }

    /// Loads a configuration from `08H` dump bytes, as one undoable step.
    #[wasm_bindgen(js_name = loadConfigDump)]
    pub fn load_config_dump(&mut self, bytes: &[u8]) -> Result<JsValue, JsError> {
        match es9_protocol::decode(bytes) {
            Ok(es9_protocol::Incoming::Config(c)) => Ok(self.dispatch(Action::LoadConfig(c))),
            Ok(_) => Err(JsError::new("not a configuration dump")),
            Err(e) => Err(JsError::new(&e.to_string())),
        }
    }

    /// Resets to one of the two factory default sets.
    ///
    /// The mock has no firmware to do this itself, so it applies the reconstruction in
    /// `es9_device::presets`. On hardware the module performs the reset and reports the
    /// result, which is authoritative where this is only a faithful copy.
    #[wasm_bindgen(js_name = resetDefaults)]
    pub fn reset_defaults(&mut self, standalone: bool) -> JsValue {
        let config = if standalone {
            mixer_defaults::standalone()
        } else {
            mixer_defaults::hosted()
        };
        self.dispatch(Action::LoadConfig(Box::new(config)))
    }

    /// Sets the DC offset trim for one analogue output.
    #[wasm_bindgen(js_name = setDcOffset)]
    pub fn set_dc_offset(&mut self, channel: u8, offset: i16) -> JsValue {
        self.dispatch(Action::SetDcOffset { channel, offset })
    }

    /// Sets the MIDI channels used for CC control. Zero disables a port.
    #[wasm_bindgen(js_name = setMidiChannels)]
    pub fn set_midi_channels(&mut self, usb: u8, din: u8) -> JsValue {
        self.dispatch(Action::SetMidiChannels { usb, din })
    }

    /// Reverses the most recent change.
    #[wasm_bindgen(js_name = undo)]
    pub fn undo(&mut self) -> bool {
        match self.session.undo() {
            Some(applied) => {
                self.pump(applied.messages);
                true
            }
            None => false,
        }
    }

    /// Reapplies the most recently undone change.
    #[wasm_bindgen(js_name = redo)]
    pub fn redo(&mut self) -> bool {
        match self.session.redo() {
            Some(applied) => {
                self.pump(applied.messages);
                true
            }
            None => false,
        }
    }

    /// Loads a flash slot for editing. `standalone` selects slot 0.
    ///
    /// This is the guarded workflow: the slot is restored before anything is edited, so
    /// what you change is what you think you are changing.
    #[wasm_bindgen(js_name = beginEdit)]
    pub fn begin_edit(&mut self, standalone: bool) {
        let target = if standalone {
            EditTarget::Standalone
        } else {
            EditTarget::Hosted
        };
        let messages = self.session.begin_edit(target);
        self.pump(messages);
    }

    /// Writes the current configuration back to the slot being edited.
    #[wasm_bindgen(js_name = save)]
    pub fn save(&mut self) {
        let message = self.session.save();
        self.pump(vec![message]);
    }

    /// The traffic log, newest last.
    #[wasm_bindgen(js_name = monitor)]
    pub fn monitor(&self) -> JsValue {
        let rows: Vec<LogRow> = self
            .monitor
            .entries()
            .map(|e| LogRow {
                seq: e.seq,
                direction: e.direction,
                at_ms: e.at_ms,
                hex: e.hex(),
                description: e.describe(),
            })
            .collect();
        to_js(&rows)
    }

    /// Empties the traffic log.
    #[wasm_bindgen(js_name = clearMonitor)]
    pub fn clear_monitor(&mut self) {
        self.monitor.clear();
    }
}

/// Renders a CC assignment the same way the map table does.
fn describe(
    _map: &es9_protocol::ccmap::CcMap,
    _cc: u8,
    assignment: Option<es9_protocol::ccmap::CcAssignment>,
) -> String {
    let Some(a) = assignment else {
        return "unassigned".to_string();
    };
    let mix = if a.mix.stereo {
        format!("Mix {}/{}", a.mix.first + 1, a.mix.first + 2)
    } else {
        format!("Mix {}", a.mix.first + 1)
    };
    let ch = if a.channel.stereo {
        format!("ch {}/{}", a.channel.first + 1, a.channel.first + 2)
    } else {
        format!("ch {}", a.channel.first + 1)
    };
    let control = match a.control {
        Control::Level => "level",
        Control::Pan => "pan",
    };
    format!("{mix}, {ch}, {control}")
}

fn to_js<T: Serialize>(value: &T) -> JsValue {
    serde_wasm_bindgen::to_value(value).unwrap_or(JsValue::NULL)
}
