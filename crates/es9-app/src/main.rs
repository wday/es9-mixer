//! Desktop shell for the ES-9 mixer.
//!
//! Joins the three layers that were built and tested separately: the device model and its
//! undo history (`es9-device`), the MIDI transport (`es9-midi`), and audio metering
//! (`es9-audio`). The frontend in `web/` is the same one the browser prototype uses; it
//! picks this backend over the WASM mock at load time.
//!
//! Two things here are not incidental:
//!
//! - **All MIDI traffic goes through one place.** Commands hand messages to the link and
//!   a single reader thread folds replies back into the model, so the monitor and the
//!   undo stack cannot be bypassed.
//! - **The module talks back.** A macro write draws an unsolicited full mix dump
//!   (hardware finding Q6), so the reader has to absorb state the host itself caused
//!   without treating it as a user edit.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(unsafe_code)]

mod presets;

use std::sync::{Arc, Mutex};

use es9_audio::capture::{Capture, MeterSnapshot};
use es9_device::action::{Action, Session};
use es9_device::monitor::{Direction, Monitor};
use es9_device::state::EditTarget;
use es9_device::view;
use es9_midi::{MidiLink, PortInfo};
use es9_protocol::{Incoming, decode, encode};
use serde::Serialize;
use tauri::State;

/// How long to wait for the module to answer one request.
///
/// A configuration dump is 2313 bytes and arrives in fragments, so this is generous.
const REPLY_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(2000);

/// What came back from the module in one drain.
#[derive(Debug, Default, Clone, Copy)]
struct Arrived {
    config: bool,
    mix: bool,
    message: bool,
    sample_rate: bool,
}

/// Sample rate to ask the audio device for.
///
/// The module reports its own rate over SysEx, and the stream is opened to match once
/// that is known; this is only the opening guess before the module has answered.
const DEFAULT_RATE: u32 = 48_000;

/// Everything the shell owns, behind one lock.
struct App {
    session: Session,
    link: Option<MidiLink>,
    /// Traffic log used before a link exists; once connected the link owns the real one.
    monitor: Monitor,
    capture: Option<Capture>,
    /// Why audio is unavailable, if it is. Shown rather than swallowed.
    capture_error: Option<String>,
}

type Shared = Arc<Mutex<App>>;

impl App {
    /// Sends messages and folds any immediate replies back into the model.
    fn pump(&mut self, messages: Vec<Vec<u8>>) {
        let Some(link) = self.link.as_mut() else {
            return;
        };
        for message in &messages {
            if let Err(e) = link.send(message) {
                eprintln!("MIDI send failed: {e}");
            }
        }
        self.drain();
    }

    /// Folds whatever the module has sent into the model, reporting what arrived.
    ///
    /// This includes the module's answers to our own writes: a macro write draws a full
    /// mix dump back. Ingesting it is correct — it is the module's own account of its
    /// state — and it must not be mistaken for a user edit, which is why nothing here
    /// touches the undo stack.
    fn drain(&mut self) -> Arrived {
        let mut arrived = Arrived::default();
        let Some(link) = self.link.as_mut() else {
            return arrived;
        };
        for message in link.poll() {
            match decode(&message) {
                Ok(incoming) => {
                    match &incoming {
                        Incoming::Config(_) => arrived.config = true,
                        Incoming::Mix(_) => arrived.mix = true,
                        Incoming::Message(_) => arrived.message = true,
                        Incoming::SampleRate(_) => arrived.sample_rate = true,
                        _ => {}
                    }
                    self.session.state.ingest(incoming);
                }
                Err(e) => eprintln!("undecodable message from module: {e}"),
            }
        }
        arrived
    }

    /// Sends one request and waits for the reply it asks for.
    ///
    /// **One request at a time.** The protocol has no flow control and a configuration
    /// dump is 2313 bytes — two orders of magnitude past a normal MIDI message — so
    /// firing every request together loses replies. Requesting serially is how the
    /// hardware probe reads a dump reliably.
    fn request(&mut self, message: Vec<u8>, wanted: fn(&Arrived) -> bool) -> bool {
        let Some(link) = self.link.as_mut() else {
            return false;
        };
        if let Err(e) = link.send(&message) {
            eprintln!("MIDI send failed: {e}");
            return false;
        }
        let deadline = std::time::Instant::now() + REPLY_TIMEOUT;
        while std::time::Instant::now() < deadline {
            if wanted(&self.drain()) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        false
    }

    /// Reads everything worth knowing from the module, in order.
    fn load_state(&mut self) {
        if !self.request(encode::request_version(), |a| a.message) {
            eprintln!("no version reply");
        }
        if !self.request(encode::request_sample_rate(), |a| a.sample_rate) {
            eprintln!("no sample rate reply");
        }
        if !self.request(encode::request_config(), |a| a.config) {
            eprintln!("NO CONFIGURATION DUMP — the mixer will show defaults, not the module");
        }
        if !self.request(encode::request_mix(), |a| a.mix) {
            eprintln!("no mix dump");
        }
        // A one-line account of what was actually loaded. Worth keeping: "the app opened"
        // and "the app is showing your module" are different claims, and only this
        // distinguishes them.
        let c = &self.session.state.config;
        eprintln!(
            "loaded: version {:?}, mixer sources {:?}, mix 1 out {:?}, hpf {:#04X}",
            self.session.state.identity.version,
            c.capture[2],
            c.outputs[2][0],
            c.hpf as u8
        );
    }

    /// Re-reads the configuration and checks it matches what was just written.
    ///
    /// The `09H` upload has no acknowledgement, so this is the only way to know it landed.
    fn verify_config(&mut self, expected: &es9_protocol::Config, what: &str) -> Result<(), String> {
        std::thread::sleep(std::time::Duration::from_millis(300));
        if !self.request(encode::request_config(), |a| a.config) {
            return Err(format!("{what} was sent but the module did not report back"));
        }
        let _ = self.request(encode::request_mix(), |a| a.mix);
        if self.session.state.config == *expected {
            Ok(())
        } else {
            Err(format!(
                "{what} did not apply cleanly — the module's configuration differs from what was sent"
            ))
        }
    }

    /// The traffic log to read: the link's when connected, otherwise the empty local one.
    ///
    /// Deliberately borrowed rather than copied — `drain` runs on every snapshot, and
    /// cloning a five-hundred-entry log at that rate would be pure waste.
    fn log(&self) -> &Monitor {
        match self.link.as_ref() {
            Some(link) => link.monitor(),
            None => &self.monitor,
        }
    }
}

// ---------------------------------------------------------------- serialised shapes

/// The full UI state.
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
    connected: bool,
    audio_error: Option<String>,
}

#[derive(Serialize)]
struct Choice {
    wire: u8,
    label: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LinkRow {
    family: u8,
    even_index: u8,
    label: String,
    linked: bool,
}

/// One logged message, shaped exactly as the WASM bridge sends it so the frontend's
/// monitor rendering is identical under both backends.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LogRow {
    seq: u64,
    direction: Direction,
    at_ms: u64,
    hex: String,
    description: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CcChangeRow {
    cc: u8,
    before: Option<String>,
    after: Option<String>,
}

/// MIDI ports, and whether a link is already open.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Ports {
    inputs: Vec<Port>,
    outputs: Vec<Port>,
    connected: bool,
    mock: bool,
}

#[derive(Serialize)]
struct Port {
    index: usize,
    name: String,
}

fn ports_of(list: Vec<PortInfo>) -> Vec<Port> {
    list.into_iter()
        .map(|p| Port {
            index: p.index,
            name: p.name,
        })
        .collect()
}

fn family_index(f: es9_protocol::tables::Family) -> u8 {
    use es9_protocol::tables::Family::*;
    match f {
        UsbLow => 0,
        UsbHigh => 1,
        Mix => 2,
        SpdifOrMix2 => 3,
        Bus => 4,
        Input => 5,
    }
}

fn family_from_index(i: u8) -> Option<es9_protocol::tables::Family> {
    use es9_protocol::tables::Family::*;
    Some(match i {
        0 => UsbLow,
        1 => UsbHigh,
        2 => Mix,
        3 => SpdifOrMix2,
        4 => Bus,
        5 => Input,
        _ => return None,
    })
}

// ---------------------------------------------------------------- commands

#[tauri::command]
fn ports(state: State<'_, Shared>) -> Ports {
    let connected = state.lock().map(|a| a.link.is_some()).unwrap_or(false);
    let (inputs, outputs) = es9_midi::list_ports().unwrap_or_default();
    Ports {
        inputs: ports_of(inputs),
        outputs: ports_of(outputs),
        connected,
        mock: false,
    }
}

/// Opens a MIDI link and loads the module's current state.
///
/// The ES-9 is published through Windows MIDI Services under a generic jack name with no
/// "ES-9" in it, so a name match cannot find it and the caller passes indices.
#[tauri::command]
fn connect(state: State<'_, Shared>, input: usize, output: usize) -> Result<(), String> {
    let link = MidiLink::open(input, output).map_err(|e| e.to_string())?;
    let mut app = state.lock().map_err(|_| "state poisoned".to_string())?;
    app.link = Some(link);
    app.session = Session::default();
    app.load_state();
    Ok(())
}

#[tauri::command]
fn snapshot(state: State<'_, Shared>) -> Result<Snapshot, String> {
    let mut app = state.lock().map_err(|_| "state poisoned".to_string())?;
    let _ = app.drain();
    let connected = app.link.is_some();
    let audio_error = app.capture_error.clone();
    let s = &app.session.state;
    Ok(Snapshot {
        status: view::status(s),
        mixes: view::mixer(s),
        routing: view::routing(s),
        dc_filters: view::dc_filters(s),
        dc_offsets: view::dc_offsets(s),
        cc_rows: view::cc_rows(s),
        links: view::links(s)
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
        can_undo: app.session.can_undo(),
        can_redo: app.session.can_redo(),
        connected,
        audio_error,
    })
}

#[tauri::command]
fn meters(state: State<'_, Shared>) -> Option<MeterSnapshot> {
    let app = state.lock().ok()?;
    app.capture.as_ref().map(|c| c.snapshot())
}

#[tauri::command]
fn monitor(state: State<'_, Shared>) -> Result<Vec<LogRow>, String> {
    let mut app = state.lock().map_err(|_| "state poisoned".to_string())?;
    let _ = app.drain();
    Ok(app
        .log()
        .entries()
        .map(|e| LogRow {
            seq: e.seq,
            direction: e.direction,
            at_ms: e.at_ms,
            hex: e.hex(),
            description: e.describe(),
        })
        .collect())
}

#[tauri::command]
fn clear_monitor(state: State<'_, Shared>) -> Result<(), String> {
    let mut app = state.lock().map_err(|_| "state poisoned".to_string())?;
    app.monitor.clear();
    if let Some(link) = app.link.as_mut() {
        link.monitor_mut().clear();
    }
    Ok(())
}

/// Applies an action, sends it, and reports any CC meanings it rewrote.
fn dispatch(state: &State<'_, Shared>, action: Action) -> Result<Vec<CcChangeRow>, String> {
    let mut app = state.lock().map_err(|_| "state poisoned".to_string())?;
    let applied = app.session.dispatch(action);
    let rows: Vec<CcChangeRow> = applied
        .cc_changes
        .iter()
        .map(|c| CcChangeRow {
            cc: c.cc,
            before: c.before.map(|_| describe(c.before)),
            after: c.after.map(|_| describe(c.after)),
        })
        .collect();
    app.pump(applied.messages);
    Ok(rows)
}

/// Renders a CC assignment the way the WASM bridge does, so the banner a link toggle
/// raises reads identically in both builds.
fn describe(assignment: Option<es9_protocol::ccmap::CcAssignment>) -> String {
    use es9_protocol::ccmap::Control;
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

#[tauri::command]
fn set_macro(state: State<'_, Shared>, cc: u8, value: u8) -> Result<Vec<CcChangeRow>, String> {
    dispatch(&state, Action::SetMacro { cc, value })
}

#[tauri::command]
fn set_raw_mix(
    state: State<'_, Shared>,
    mix: u8,
    channel: u8,
    value: u16,
) -> Result<Vec<CcChangeRow>, String> {
    dispatch(&state, Action::SetRawMix { mix, channel, value })
}

#[tauri::command]
fn set_capture(
    state: State<'_, Shared>,
    block: u8,
    channel: u8,
    wire: u8,
) -> Result<Vec<CcChangeRow>, String> {
    dispatch(
        &state,
        Action::SetCapture {
            block,
            channel,
            source: wire,
        },
    )
}

#[tauri::command]
fn set_output(
    state: State<'_, Shared>,
    block: u8,
    channel: u8,
    wire: u8,
) -> Result<Vec<CcChangeRow>, String> {
    dispatch(
        &state,
        Action::SetOutput {
            block,
            channel,
            destination: wire,
        },
    )
}

#[tauri::command]
fn set_link(
    state: State<'_, Shared>,
    family: u8,
    even_index: u8,
    on: bool,
) -> Result<Vec<CcChangeRow>, String> {
    let family = family_from_index(family).ok_or_else(|| "unknown family".to_string())?;
    dispatch(
        &state,
        Action::SetLink {
            family,
            even_index,
            on,
        },
    )
}

#[tauri::command]
fn set_options(
    state: State<'_, Shared>,
    mixer2: bool,
    midi_thru: bool,
) -> Result<Vec<CcChangeRow>, String> {
    dispatch(
        &state,
        Action::SetOptions(es9_protocol::config::Options { mixer2, midi_thru }),
    )
}

#[tauri::command]
fn set_smoothing(state: State<'_, Shared>, mix: u8, on: bool) -> Result<Vec<CcChangeRow>, String> {
    dispatch(&state, Action::SetSmoothing { mix, on })
}

#[tauri::command]
fn set_dc_block(state: State<'_, Shared>, bit: u8, on: bool) -> Result<Vec<CcChangeRow>, String> {
    dispatch(&state, Action::SetDcBlock { bit, on })
}

#[tauri::command]
fn set_dc_offset(
    state: State<'_, Shared>,
    channel: u8,
    offset: i16,
) -> Result<Vec<CcChangeRow>, String> {
    dispatch(&state, Action::SetDcOffset { channel, offset })
}

#[tauri::command]
fn set_midi_channels(
    state: State<'_, Shared>,
    usb: u8,
    din: u8,
) -> Result<Vec<CcChangeRow>, String> {
    dispatch(&state, Action::SetMidiChannels { usb, din })
}

// ---------------------------------------------------------------- presets

#[tauri::command]
fn preset_list() -> Vec<presets::PresetInfo> {
    presets::list()
}

/// Saves the module's current configuration under a name.
#[tauri::command]
fn preset_save(state: State<'_, Shared>, name: String) -> Result<(), String> {
    let app = state.lock().map_err(|_| "state poisoned".to_string())?;
    presets::save(&name, &app.session.state.config)
}

/// Loads a preset into the module, and verifies that it took.
///
/// The upload is 24 chunks with no acknowledgement, so the configuration is read back and
/// compared rather than assumed. H8 measured this as reliable, but "measured once" is not
/// "guaranteed", and a preset that half-applied would be worse than one that failed.
#[tauri::command]
fn preset_load(state: State<'_, Shared>, name: String) -> Result<(), String> {
    let target = presets::load(&name)?;
    let mut app = state.lock().map_err(|_| "state poisoned".to_string())?;
    let applied = app
        .session
        .dispatch(Action::LoadConfig(Box::new(target.clone())));
    app.pump(applied.messages);
    app.verify_config(&target, "preset")
}

#[tauri::command]
fn preset_delete(name: String) -> Result<(), String> {
    presets::delete(&name)
}

/// Resets the current configuration to one of the module's two factory default sets.
///
/// The module does this itself (`26H`), so there is no action to invert — the previous
/// configuration is captured first and recorded as the undo step. It does **not** touch
/// either flash slot: saving is still a separate, deliberate act.
#[tauri::command]
fn reset_defaults(state: State<'_, Shared>, standalone: bool) -> Result<(), String> {
    let mut app = state.lock().map_err(|_| "state poisoned".to_string())?;
    let previous = Box::new(app.session.state.config.clone());
    let target = if standalone {
        es9_protocol::encode::FlashSlot::Standalone
    } else {
        es9_protocol::encode::FlashSlot::Hosted
    };
    app.pump(vec![encode::reset_to_defaults(target)]);
    std::thread::sleep(std::time::Duration::from_millis(300));
    if !app.request(encode::request_config(), |a| a.config) {
        return Err("the module did not report its configuration after the reset".into());
    }
    let _ = app.request(encode::request_mix(), |a| a.mix);
    app.session.record_undo(Action::LoadConfig(previous));
    app.session.state.dirty = true;
    Ok(())
}

#[tauri::command]
fn undo(state: State<'_, Shared>) -> Result<bool, String> {
    let mut app = state.lock().map_err(|_| "state poisoned".to_string())?;
    match app.session.undo() {
        Some(applied) => {
            app.pump(applied.messages);
            Ok(true)
        }
        None => Ok(false),
    }
}

#[tauri::command]
fn redo(state: State<'_, Shared>) -> Result<bool, String> {
    let mut app = state.lock().map_err(|_| "state poisoned".to_string())?;
    match app.session.redo() {
        Some(applied) => {
            app.pump(applied.messages);
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Switches which flash slot is being edited, loading it first.
///
/// Connecting over USB forces the module into hosted mode, so editing the standalone
/// configuration without restoring it first silently edits hosted state. `begin_edit`
/// sends the restore before anything else, which is what makes that impossible.
#[tauri::command]
fn begin_edit(state: State<'_, Shared>, standalone: bool) -> Result<(), String> {
    let mut app = state.lock().map_err(|_| "state poisoned".to_string())?;
    let target = if standalone {
        EditTarget::Standalone
    } else {
        EditTarget::Hosted
    };
    let messages = app.session.begin_edit(target);
    app.pump(messages);
    Ok(())
}

#[tauri::command]
fn save(state: State<'_, Shared>) -> Result<(), String> {
    let mut app = state.lock().map_err(|_| "state poisoned".to_string())?;
    let message = app.session.save();
    app.pump(vec![message]);
    Ok(())
}

// ---------------------------------------------------------------- startup

fn main() {
    let app = Arc::new(Mutex::new(App {
        session: Session::default(),
        link: None,
        monitor: Monitor::new(500),
        capture: None,
        capture_error: None,
    }));

    tauri::Builder::default()
        .manage(Arc::clone(&app))
        .setup(move |handle| {
            let shared = Arc::clone(&app);
            // Auto-connect on startup so the common case needs no interaction. The ES-9
            // cannot be found by name on Windows, so this tries every output against
            // every input and keeps the pair that answers a version request.
            std::thread::spawn(move || {
                match autoconnect(&shared) {
                    Ok(Some((i, o))) => eprintln!("connected: input {i}, output {o}"),
                    Ok(None) => eprintln!("no ES-9 answered on any MIDI port pair"),
                    Err(e) => eprintln!("MIDI startup failed: {e}"),
                }
                // Audio is independent of MIDI: meters are worth having even if the MIDI
                // link never comes up, and vice versa.
                match Capture::open(DEFAULT_RATE) {
                    Ok(capture) => {
                        eprintln!(
                            "metering {} channels at {} Hz via {}",
                            capture.channels, capture.sample_rate, capture.host
                        );
                        if let Ok(mut a) = shared.lock() {
                            a.capture = Some(capture);
                        }
                    }
                    Err(e) => {
                        eprintln!("no metering: {e}");
                        if let Ok(mut a) = shared.lock() {
                            a.capture_error = Some(e.to_string());
                        }
                    }
                }
            });
            let _ = handle;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ports,
            connect,
            snapshot,
            meters,
            monitor,
            clear_monitor,
            set_macro,
            set_raw_mix,
            set_capture,
            set_output,
            set_link,
            set_options,
            set_smoothing,
            set_dc_block,
            set_dc_offset,
            preset_list,
            preset_save,
            preset_load,
            preset_delete,
            reset_defaults,
            set_midi_channels,
            undo,
            redo,
            begin_edit,
            save,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start the ES-9 mixer");
}

/// Finds the ES-9 by asking, since it cannot be found by name.
///
/// Windows publishes the module through Windows MIDI Services under a generic jack name
/// ("2 - MIDI In"), so matching on "ES-9" finds nothing. Every plausible pair is opened
/// and sent a version request; the one that answers with an ES-9 message is the module.
fn autoconnect(shared: &Shared) -> Result<Option<(usize, usize)>, String> {
    let (inputs, outputs) = es9_midi::list_ports().map_err(|e| e.to_string())?;

    for out in &outputs {
        for inp in &inputs {
            let Ok(mut link) = MidiLink::open(inp.index, out.index) else {
                continue;
            };
            if link.send(&encode::request_version()).is_err() {
                continue;
            }
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(400);
            while std::time::Instant::now() < deadline {
                for message in link.poll() {
                    if let Ok(Incoming::Message(_)) = decode(&message) {
                        let mut app = shared.lock().map_err(|_| "state poisoned".to_string())?;
                        app.link = Some(link);
                        app.load_state();
                        return Ok(Some((inp.index, out.index)));
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }
    Ok(None)
}
