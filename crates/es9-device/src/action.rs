//! Actions: the only way device state changes.
//!
//! Applying an action mutates the model, produces the MIDI bytes that make the module
//! agree, and yields an inverse for undo. Nothing bypasses this, so the model, the wire,
//! and the history stay consistent by construction.

use es9_protocol::ccmap::CcChange;
use es9_protocol::config::{EqBand, Options};
use es9_protocol::encode;
use es9_protocol::links::Links;
use es9_protocol::tables::{Family, Source};

use crate::state::{DeviceState, EditTarget};

/// A single change to the device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Set a macro mix control, addressed by its MIDI CC number.
    SetMacro {
        /// CC number, which is also the module's internal index.
        cc: u8,
        /// New 7-bit value.
        value: u8,
    },
    /// Set one raw mixer gain.
    SetRawMix {
        /// Zero-based mix.
        mix: u8,
        /// Zero-based channel.
        channel: u8,
        /// New 16-bit gain.
        value: u16,
    },
    /// Point one capture channel at a source, honouring stereo links.
    SetCapture {
        /// Routing block, 0-3.
        block: u8,
        /// Channel within the block, 0-7.
        channel: u8,
        /// Source wire byte.
        source: u8,
    },
    /// Replace a whole capture block. Used as the inverse of [`Action::SetCapture`].
    SetCaptureBlock {
        /// Routing block, 0-3.
        block: u8,
        /// Eight source wire bytes.
        sources: [u8; 8],
    },
    /// Point one output channel at a destination.
    SetOutput {
        /// Routing block, 0-3.
        block: u8,
        /// Channel within the block, 0-7.
        channel: u8,
        /// Destination wire byte.
        destination: u8,
    },
    /// Replace a whole output block.
    SetOutputBlock {
        /// Routing block, 0-3.
        block: u8,
        /// Eight destination wire bytes.
        destinations: [u8; 8],
    },
    /// Enable or disable a stereo link. This also rewrites the MIDI CC map.
    SetLink {
        /// Signal family the pair belongs to.
        family: Family,
        /// Even channel index starting the pair.
        even_index: u8,
        /// Whether the pair is linked.
        on: bool,
    },
    /// Set the global options word.
    SetOptions(Options),
    /// Enable or disable gain smoothing for one mix.
    SetSmoothing {
        /// Zero-based mix.
        mix: u8,
        /// Whether smoothing is enabled.
        on: bool,
    },
    /// Set the DC-blocking filter bitmap.
    SetHpf(u8),
    /// Turn the DC-blocking filter for one input pair on or off.
    ///
    /// Separate from [`Action::SetHpf`] so that undo restores one filter rather than the
    /// whole bitmap, and so callers never have to do the bit arithmetic themselves.
    SetDcBlock {
        /// Bit index, addressing a pair in `es9_protocol::dcblock::PAIRS`.
        bit: u8,
        /// Whether the filter is engaged.
        on: bool,
    },
    /// Set the MIDI channels used for CC control.
    SetMidiChannels {
        /// Channel for the USB port; 0 disables.
        usb: u8,
        /// Channel for the DIN port; 0 disables.
        din: u8,
    },
    /// Set an output DC offset.
    SetDcOffset {
        /// Output channel, 0-7.
        channel: u8,
        /// Signed offset.
        offset: i16,
    },
    /// Set one band of input EQ.
    SetFilter {
        /// EQ block, 0 or 1.
        block: u8,
        /// Channel, 0-7.
        channel: u8,
        /// Band, 0-3.
        band: u8,
        /// The band's parameters.
        value: EqBand,
    },
    /// Several actions applied as one undoable step.
    /// Replace the entire configuration, as when loading a preset.
    ///
    /// Sent as the `09H` chunked upload, which was measured on hardware to round-trip
    /// exactly (checklist H8). The inverse is the configuration that was there before, so
    /// loading a preset is a single undoable step rather than a point of no return.
    LoadConfig(Box<es9_protocol::Config>),
    /// Several actions applied and undone as one step.
    Batch(Vec<Action>),
}

/// The result of applying an action.
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    /// Messages to send to the module, in order.
    pub messages: Vec<Vec<u8>>,
    /// The action that undoes this one.
    pub inverse: Action,
    /// CC numbers whose meaning changed, when a link or option rewrote the map.
    ///
    /// Empty for most actions. A single stereo link toggle can change sixteen entries,
    /// which is exactly the surprise the module gives no warning about.
    pub cc_changes: Vec<CcChange>,
}

impl DeviceState {
    /// Applies an action, returning the messages to send and the inverse for undo.
    pub fn apply(&mut self, action: &Action) -> Applied {
        let before_map = self.cc_map();
        let (messages, inverse) = self.apply_inner(action);
        let after_map = self.cc_map();
        let cc_changes = before_map.diff(&after_map);
        self.dirty = true;
        Applied {
            messages,
            inverse,
            cc_changes,
        }
    }

    fn apply_inner(&mut self, action: &Action) -> (Vec<Vec<u8>>, Action) {
        match *action {
            Action::SetMacro { cc, value } => {
                let previous = self.macro_values[usize::from(cc)];
                self.macro_values[usize::from(cc)] = value;
                (
                    vec![encode::set_macro(cc, value)],
                    Action::SetMacro {
                        cc,
                        value: previous,
                    },
                )
            }
            Action::SetRawMix {
                mix,
                channel,
                value,
            } => {
                let previous = self.config.raw_mix[usize::from(mix)][usize::from(channel)];
                self.config.raw_mix[usize::from(mix)][usize::from(channel)] = value;
                (
                    vec![encode::set_raw_mix(mix, channel, value)],
                    Action::SetRawMix {
                        mix,
                        channel,
                        value: previous,
                    },
                )
            }
            Action::SetCapture {
                block,
                channel,
                source,
            } => {
                let b = usize::from(block);
                let previous = self.config.capture[b];
                self.set_capture_linked(block, channel, source);
                (
                    vec![encode::set_capture(block, self.config.capture[b])],
                    Action::SetCaptureBlock {
                        block,
                        sources: previous,
                    },
                )
            }
            Action::SetCaptureBlock { block, sources } => {
                let b = usize::from(block);
                let previous = self.config.capture[b];
                self.config.capture[b] = sources;
                (
                    vec![encode::set_capture(block, sources)],
                    Action::SetCaptureBlock {
                        block,
                        sources: previous,
                    },
                )
            }
            Action::SetOutput {
                block,
                channel,
                destination,
            } => {
                let b = usize::from(block);
                let previous = self.config.outputs[b];
                self.config.outputs[b][usize::from(channel)] = destination;
                (
                    vec![encode::set_outputs(block, self.config.outputs[b])],
                    Action::SetOutputBlock {
                        block,
                        destinations: previous,
                    },
                )
            }
            Action::SetOutputBlock {
                block,
                destinations,
            } => {
                let b = usize::from(block);
                let previous = self.config.outputs[b];
                self.config.outputs[b] = destinations;
                (
                    vec![encode::set_outputs(block, destinations)],
                    Action::SetOutputBlock {
                        block,
                        destinations: previous,
                    },
                )
            }
            Action::SetLink {
                family,
                even_index,
                on,
            } => {
                let previous = self.config.links.is_linked(family, even_index);
                self.config.links.set(family, even_index, on);
                let index = Links::message_index(family, even_index).unwrap_or(0);
                (
                    vec![encode::set_link(index, on)],
                    Action::SetLink {
                        family,
                        even_index,
                        on: previous,
                    },
                )
            }
            Action::SetOptions(options) => {
                let previous = self.config.options;
                self.config.options = options;
                (
                    vec![encode::set_options(options)],
                    Action::SetOptions(previous),
                )
            }
            Action::SetSmoothing { mix, on } => {
                let bit = 1u16 << mix;
                let previous = self.config.smoothing & bit != 0;
                if on {
                    self.config.smoothing |= bit;
                } else {
                    self.config.smoothing &= !bit;
                }
                (
                    vec![encode::set_smoothing(mix, on)],
                    Action::SetSmoothing { mix, on: previous },
                )
            }
            Action::SetHpf(bits) => {
                let previous = self.config.hpf as u8;
                self.config.hpf = u32::from(bits);
                (vec![encode::set_hpf(bits)], Action::SetHpf(previous))
            }
            Action::SetDcBlock { bit, on } => {
                let previous = self.config.hpf as u8;
                let was_on = es9_protocol::dcblock::is_on(previous, bit);
                let bits = es9_protocol::dcblock::with(previous, bit, on);
                self.config.hpf = u32::from(bits);
                // The module has no per-pair command, so the whole bitmap goes out; the
                // inverse is still per-pair, so undo touches one filter.
                (
                    vec![encode::set_hpf(bits)],
                    Action::SetDcBlock { bit, on: was_on },
                )
            }
            Action::SetMidiChannels { usb, din } => {
                let previous = (self.config.midi_usb, self.config.midi_din);
                self.config.midi_usb = usb;
                self.config.midi_din = din;
                (
                    vec![encode::set_midi_channels(usb, din)],
                    Action::SetMidiChannels {
                        usb: previous.0,
                        din: previous.1,
                    },
                )
            }
            Action::SetDcOffset { channel, offset } => {
                let previous = self.config.dc_offsets[usize::from(channel)];
                self.config.dc_offsets[usize::from(channel)] = offset;
                (
                    vec![encode::set_dc_offset(channel, offset)],
                    Action::SetDcOffset {
                        channel,
                        offset: previous,
                    },
                )
            }
            Action::SetFilter {
                block,
                channel,
                band,
                value,
            } => {
                let previous =
                    self.config.eq[usize::from(block)][usize::from(channel)][usize::from(band)];
                self.config.eq[usize::from(block)][usize::from(channel)][usize::from(band)] = value;
                (
                    vec![encode::set_filter(block, channel, band, value)],
                    Action::SetFilter {
                        block,
                        channel,
                        band,
                        value: previous,
                    },
                )
            }
            Action::LoadConfig(ref config) => {
                let previous = Box::new(self.config.clone());
                self.config = (**config).clone();
                (encode::config_upload(config), Action::LoadConfig(previous))
            }
            Action::Batch(ref actions) => {
                let mut messages = Vec::new();
                let mut inverses = Vec::with_capacity(actions.len());
                for a in actions {
                    let (mut m, inv) = self.apply_inner(a);
                    messages.append(&mut m);
                    inverses.push(inv);
                }
                // Undo in the opposite order to application.
                inverses.reverse();
                (messages, Action::Batch(inverses))
            }
        }
    }

    /// Sets a capture channel, dragging its partner along when the source is stereo.
    ///
    /// This mirrors the reference tool: assigning one half of a linked pair assigns the
    /// other half to the adjacent source, so a stereo signal stays intact.
    fn set_capture_linked(&mut self, block: u8, channel: u8, source: u8) {
        let b = usize::from(block);
        self.config.capture[b][usize::from(channel)] = source;

        let Some((family, index)) = Source::from_wire(source).and_then(Source::family_index) else {
            return;
        };
        if !self.config.links.in_stereo_pair(family, index) {
            return;
        }
        let even = index & !1;
        let base_channel = channel & !1;
        for offset in 0..2u8 {
            let Some(partner) = Source::from_family_index(family, even + offset) else {
                continue;
            };
            let Some(wire) = partner.to_wire() else {
                continue;
            };
            if let Some(slot) = self.config.capture[b].get_mut(usize::from(base_channel + offset)) {
                *slot = wire;
            }
        }
    }
}

/// State plus undo history, and the guarded flash-editing workflow.
#[derive(Debug, Clone, Default)]
pub struct Session {
    /// The device model.
    pub state: DeviceState,
    undo_stack: Vec<Action>,
    redo_stack: Vec<Action>,
}

impl Session {
    /// Applies an action and records it for undo, discarding any redo history.
    pub fn dispatch(&mut self, action: Action) -> Applied {
        let applied = self.state.apply(&action);
        self.undo_stack.push(applied.inverse.clone());
        self.redo_stack.clear();
        applied
    }

    /// Reverses the most recent action.
    pub fn undo(&mut self) -> Option<Applied> {
        let inverse = self.undo_stack.pop()?;
        let applied = self.state.apply(&inverse);
        self.redo_stack.push(applied.inverse.clone());
        Some(applied)
    }

    /// Reapplies the most recently undone action.
    pub fn redo(&mut self) -> Option<Applied> {
        let inverse = self.redo_stack.pop()?;
        let applied = self.state.apply(&inverse);
        self.undo_stack.push(applied.inverse.clone());
        Some(applied)
    }

    /// Whether there is anything to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Whether there is anything to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Records an inverse for something the module did on its own account.
    ///
    /// A factory reset is performed by the module (`26H`), not synthesised here, so there
    /// is no action to dispatch and no inverse to derive — but it is still destructive and
    /// still ought to be undoable. The caller captures the configuration beforehand and
    /// hands back the action that would restore it.
    pub fn record_undo(&mut self, inverse: Action) {
        self.undo_stack.push(inverse);
        self.redo_stack.clear();
    }

    /// Messages to request everything worth knowing after connecting.
    pub fn request_all() -> Vec<Vec<u8>> {
        vec![
            encode::request_version(),
            encode::request_sample_rate(),
            encode::request_config(),
            encode::request_mix(),
        ]
    }

    /// Begins editing a flash slot, loading it first.
    ///
    /// This is the guarded form of the workflow the manual describes. Connecting over USB
    /// puts the module in hosted mode, so editing the standalone configuration without
    /// restoring it first silently edits hosted state instead — the easiest way to lose a
    /// live configuration. Going through this method makes that impossible.
    pub fn begin_edit(&mut self, target: EditTarget) -> Vec<Vec<u8>> {
        self.state.editing = target;
        self.state.dirty = false;
        self.undo_stack.clear();
        self.redo_stack.clear();
        vec![
            encode::restore_from_flash(target.slot()),
            encode::request_config(),
            encode::request_mix(),
        ]
    }

    /// Saves the current configuration back to the slot being edited.
    pub fn save(&mut self) -> Vec<u8> {
        self.state.dirty = false;
        encode::save_to_flash(self.state.editing.slot())
    }
}
