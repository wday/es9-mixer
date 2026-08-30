//! Presentation model.
//!
//! The shapes a UI renders, derived from device state. Kept here rather than in the
//! frontend so the labelling and stereo-collapsing rules are testable in Rust, and so the
//! browser build and the native build show identical things.
//!
//! Mixer strips are derived from the resolved MIDI CC map, which means every control
//! carries the CC number the hardware will actually honour.

use es9_protocol::dcblock;
use es9_protocol::scales;
use es9_protocol::tables::{Destination, Family, Source};

use crate::state::{DeviceState, EditTarget};

/// Headline information about the module.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Status {
    /// Firmware version string, once known.
    pub version: Option<String>,
    /// Operating sample rate in hertz.
    pub sample_rate: Option<u32>,
    /// Which flash slot is being edited.
    pub editing: EditTarget,
    /// Whether there are changes not yet written to that slot.
    pub dirty: bool,
    /// Whether the second mixer is enabled.
    pub mixer2: bool,
    /// The S/PDIF processor, present only when block 3 is acting as S/PDIF.
    ///
    /// `None` when [`Status::mixer2`] is set, because the block is a mixer bank instead.
    pub spdif: Option<Spdif>,
    /// DSP load percentage, when reported.
    pub dsp_load: Option<f32>,
    /// The module's most recent text message.
    pub last_message: Option<String>,
}

/// What the S/PDIF processor is actually carrying.
///
/// Options bit 0 only says block 3 is *acting* as the S/PDIF processor. It says nothing
/// about whether either direction reaches anything, and a status line that reports the
/// mode alone reads as "S/PDIF works" when it may do nothing at all. Same reasoning as
/// [`DcOffset::effective`]: the mode is not the routing.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Spdif {
    /// What feeds the S/PDIF output, e.g. `"USB 5/6"`.
    ///
    /// Block 3's capture side is the S/PDIF output, so this is always carrying something.
    pub output_from: String,
    /// The USB capture channels carrying the S/PDIF input, e.g. `"capture 15/16"`.
    ///
    /// `None` means the input reaches nothing: the host cannot hear it, whatever the
    /// mode says.
    pub input_to: Option<String>,
}

/// One channel strip within a mix.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Strip {
    /// Zero-based mixer channel, or the left channel of a stereo pair.
    pub channel: u8,
    /// Whether this strip covers two channels.
    pub stereo: bool,
    /// What the strip is carrying, e.g. `"USB 1/2"`.
    pub label: String,
    /// Macro level, 0-127.
    pub level: u8,
    /// The same level in decibels; `None` means silence.
    pub level_db: Option<f32>,
    /// CC number controlling the level.
    pub level_cc: u8,
    /// Pan position, -63 to 63, when the mix is stereo.
    pub pan: Option<i8>,
    /// CC number controlling pan, when the mix is stereo.
    pub pan_cc: Option<u8>,
    /// Raw 16-bit gains for the underlying cells, behind the advanced disclosure.
    pub raw: Vec<u16>,
}

/// One mix, or a stereo pair of mixes shown as one.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Mix {
    /// Zero-based index, or the left mix of a stereo pair.
    pub index: u8,
    /// Whether this is a stereo pair.
    pub stereo: bool,
    /// Display name, e.g. `"Mix 1/2"`.
    pub name: String,
    /// Where the mix goes, e.g. `"Main Out L/R"`.
    pub destination: String,
    /// Whether gain smoothing is enabled for this mix.
    pub smoothing: bool,
    /// The channel strips feeding it.
    pub strips: Vec<Strip>,
}

/// One row of the CC map table.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct CcRow {
    /// The CC number.
    pub cc: u8,
    /// What it controls, or `None` if a stereo link has shadowed it.
    pub target: Option<String>,
}

/// One assignable channel in the routing patchbay.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct RouteChannel {
    /// Routing block, 0-3.
    pub block: u8,
    /// Channel within the block, 0-7.
    pub channel: u8,
    /// Current assignment, e.g. `"Input 3"`.
    pub label: String,
    /// The raw wire byte, for round-tripping unrecognised values.
    pub wire: u8,
    /// Whether the wire value was recognised.
    pub known: bool,
}

/// The routing view: what each block captures and where it sends.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Routing {
    /// Capture assignments for all four blocks.
    pub capture: Vec<RouteChannel>,
    /// Output assignments for all four blocks.
    pub outputs: Vec<RouteChannel>,
}

/// One output's DC offset trim.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct DcOffset {
    /// Output channel, 0-7.
    pub channel: u8,
    /// Display name, e.g. `"Output 3"`.
    pub label: String,
    /// Current offset.
    pub offset: i16,
    /// Whether a mixer is routed to this output.
    ///
    /// The offset is implemented with the mixer hardware, so on an output fed from
    /// anything else — USB playback, most commonly — writing an offset does nothing at
    /// all. The module gives no indication of this, so the app has to.
    pub effective: bool,
    /// Which mix feeds this output, when one does.
    pub fed_by: Option<String>,
}

/// Builds the output DC offset view, one entry per 3.5mm analogue output.
pub fn dc_offsets(state: &DeviceState) -> Vec<DcOffset> {
    (0..8u8)
        .map(|channel| {
            let fed_by = mix_feeding_output(state, channel + 1);
            DcOffset {
                channel,
                label: format!("Output {}", channel + 1),
                offset: state.config.dc_offsets[usize::from(channel)],
                effective: fed_by.is_some(),
                fed_by,
            }
        })
        .collect()
}

/// The mix routed to a given 1-based analogue output, if any.
///
/// Mix `m` lives at output block `m/8 + 2`, channel `m%8`; its destination says where it
/// goes. An output with no mix pointing at it cannot take a DC offset.
fn mix_feeding_output(state: &DeviceState, output: u8) -> Option<String> {
    for mix in 0..16usize {
        let block = mix / 8 + 2;
        let channel = mix % 8;
        if state.config.output_destination(block, channel) == Some(Destination::Output(output)) {
            return Some(format!("Mix {}", mix + 1));
        }
    }
    None
}

/// Describes what the S/PDIF processor is carrying in each direction.
fn spdif(state: &DeviceState) -> Spdif {
    // Which of the 16 USB capture channels are listening to the S/PDIF input. Blocks 0
    // and 1 are capture 1-8 and 9-16, so the channel number runs across both.
    let listening: Vec<u8> = (0..16u8)
        .filter(|&ch| {
            matches!(
                state
                    .config
                    .capture_source(usize::from(ch) / 8, usize::from(ch) % 8),
                Some(Source::Spdif(_))
            )
        })
        .map(|ch| ch + 1)
        .collect();

    let input_to = match listening.as_slice() {
        [] => None,
        [a, b] if *b == a + 1 => Some(format!("capture {a}/{b}")),
        chans => Some(format!(
            "capture {}",
            chans
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )),
    };

    Spdif {
        output_from: source_label(state, 3, 0, true),
        input_to,
    }
}

/// One DC-blocking filter, covering a pair of analogue inputs.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct DcFilter {
    /// Bit index in the bitmap.
    pub bit: u8,
    /// The two inputs this filter covers, as 1-based input numbers.
    pub inputs: (u8, u8),
    /// Label, e.g. `"Inputs 3/8"`.
    pub label: String,
    /// Whether the filter is engaged. On suits audio; off is required for CV.
    pub on: bool,
    /// Whether the two inputs are not consecutive, e.g. 3 and 8.
    ///
    /// The UI should say so where this is true: someone enabling the filter for input 8
    /// is also enabling it for input 3, which would wreck a CV patched there.
    pub surprising: bool,
}

/// Builds the DC-blocking view, one entry per filter.
pub fn dc_filters(state: &DeviceState) -> Vec<DcFilter> {
    let bitmap = state.config.hpf as u8;
    (0..dcblock::PAIR_COUNT as u8)
        .filter_map(|bit| {
            let (a, b) = dcblock::pair(bit)?;
            Some(DcFilter {
                bit,
                inputs: (a, b),
                label: dcblock::label(bit)?,
                on: dcblock::is_on(bitmap, bit),
                surprising: b != a + 1,
            })
        })
        .collect()
}

/// Builds the status view.
pub fn status(state: &DeviceState) -> Status {
    Status {
        version: state.identity.version.clone(),
        sample_rate: state.identity.sample_rate,
        editing: state.editing,
        dirty: state.dirty,
        mixer2: state.config.options.mixer2,
        spdif: (!state.config.options.mixer2).then(|| spdif(state)),
        dsp_load: state
            .identity
            .usage
            .map(|u| es9_protocol::decode::Usage::percent(u.dsp0)),
        last_message: state.identity.last_message.clone(),
    }
}

/// Names a source, merging a stereo pair into one label such as `"USB 1/2"`.
fn source_label(state: &DeviceState, block: usize, channel: u8, stereo: bool) -> String {
    let Some(source) = state.config.capture_source(block, usize::from(channel)) else {
        return format!(
            "? {:#04X}",
            state.config.capture[block][usize::from(channel)]
        );
    };
    if !stereo {
        return source.to_string();
    }
    let partner = source
        .family_index()
        .and_then(|(f, i)| Source::from_family_index(f, (i & !1) + 1));
    match partner {
        Some(p) => merge_labels(&source.to_string(), &p.to_string()),
        None => source.to_string(),
    }
}

/// Joins two labels compactly: `"USB 1"` and `"USB 2"` become `"USB 1/2"`.
fn merge_labels(left: &str, right: &str) -> String {
    let prefix_len = left
        .char_indices()
        .zip(right.chars())
        .take_while(|((_, a), b)| a == b)
        .map(|((i, c), _)| i + c.len_utf8())
        .last()
        .unwrap_or(0);
    if prefix_len == 0 {
        return format!("{left}/{right}");
    }
    format!("{left}/{}", &right[prefix_len..])
}

/// Builds the console mixer view.
///
/// Only mixes that exist are returned: with the second mixer disabled, mixes 9-16 are not
/// available and do not appear.
pub fn mixer(state: &DeviceState) -> Vec<Mix> {
    let map = state.cc_map();
    let mut mixes: Vec<Mix> = Vec::new();

    for strip_ref in map.strips() {
        let mix_index = strip_ref.mix.first;
        let block = usize::from(mix_index) / 8;

        if !mixes.iter().any(|m| m.index == mix_index) {
            mixes.push(Mix {
                index: mix_index,
                stereo: strip_ref.mix.stereo,
                name: if strip_ref.mix.stereo {
                    format!("Mix {}/{}", mix_index + 1, mix_index + 2)
                } else {
                    format!("Mix {}", mix_index + 1)
                },
                destination: destination_label(state, mix_index, strip_ref.mix.stereo),
                smoothing: state.config.smoothing & (1 << mix_index) != 0,
                strips: Vec::new(),
            });
        }

        let level = state.macro_values[usize::from(strip_ref.level_cc)];
        let db = scales::macro_to_db(level);
        let raw = raw_cells(state, mix_index, strip_ref.mix.stereo, strip_ref.channel);

        let strip = Strip {
            channel: strip_ref.channel.first,
            stereo: strip_ref.channel.stereo,
            label: source_label(
                state,
                block + 2,
                strip_ref.channel.first,
                strip_ref.channel.stereo,
            ),
            level,
            level_db: db.is_finite().then_some(db),
            level_cc: strip_ref.level_cc,
            // Pan is read from the *level* cell's aux byte, not from the CC it is
            // written to: the module stores it there. Reading the pan CC's own value
            // gives zero, which renders as hard left on every strip (checklist H3).
            pan: strip_ref
                .pan_cc
                .map(|_| scales::stored_to_pan(state.macro_aux[usize::from(strip_ref.level_cc)])),
            pan_cc: strip_ref.pan_cc,
            raw,
        };
        if let Some(m) = mixes.iter_mut().find(|m| m.index == mix_index) {
            m.strips.push(strip);
        }
    }
    mixes
}

/// The raw gain cells a strip covers, which may be up to four for a stereo strip on a
/// stereo mix.
fn raw_cells(
    state: &DeviceState,
    mix: u8,
    mix_stereo: bool,
    channel: es9_protocol::ccmap::ChannelRef,
) -> Vec<u16> {
    let mixes = if mix_stereo { 2 } else { 1 };
    let channels = if channel.stereo { 2 } else { 1 };
    let mut out = Vec::with_capacity(mixes * channels);
    for m in 0..mixes {
        for c in 0..channels {
            let row = usize::from(mix) + m;
            let col = usize::from(channel.first) + c;
            if let Some(v) = state.config.raw_mix.get(row).and_then(|r| r.get(col)) {
                out.push(*v);
            }
        }
    }
    out
}

fn destination_label(state: &DeviceState, mix: u8, stereo: bool) -> String {
    let block = usize::from(mix) / 8 + 2;
    let channel = usize::from(mix) % 8;
    let name = |ch: usize| {
        state
            .config
            .output_destination(block, ch)
            .map(|d| d.to_string())
            .unwrap_or_else(|| "?".to_string())
    };
    if stereo {
        merge_labels(&name(channel), &name(channel + 1))
    } else {
        name(channel)
    }
}

/// Builds the CC map table, one row per CC number.
pub fn cc_rows(state: &DeviceState) -> Vec<CcRow> {
    let map = state.cc_map();
    (0u8..128)
        .map(|cc| CcRow {
            cc,
            target: map.get(cc).map(|a| {
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
                    es9_protocol::ccmap::Control::Level => "level",
                    es9_protocol::ccmap::Control::Pan => "pan",
                };
                format!("{mix}, {ch}, {control}")
            }),
        })
        .collect()
}

/// Builds the routing patchbay view.
pub fn routing(state: &DeviceState) -> Routing {
    let mut capture = Vec::with_capacity(32);
    let mut outputs = Vec::with_capacity(32);
    for block in 0..4u8 {
        for channel in 0..8u8 {
            let wire = state.config.capture[usize::from(block)][usize::from(channel)];
            let decoded = Source::from_wire(wire);
            capture.push(RouteChannel {
                block,
                channel,
                label: decoded
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("? {wire:#04X}")),
                wire,
                known: decoded.is_some(),
            });

            let wire = state.config.outputs[usize::from(block)][usize::from(channel)];
            let decoded = Destination::from_wire(wire);
            outputs.push(RouteChannel {
                block,
                channel,
                label: decoded
                    .map(|d| d.to_string())
                    .unwrap_or_else(|| format!("? {wire:#04X}")),
                wire,
                known: decoded.is_some(),
            });
        }
    }
    Routing { capture, outputs }
}

/// Every source the patchbay can offer, in natural order.
pub fn available_sources() -> Vec<(u8, String)> {
    let all = (1..=14u8)
        .map(Source::Input)
        .chain((1..=16).map(Source::Bus))
        .chain((1..=16).map(Source::Usb))
        .chain((1..=8).map(Source::Mix))
        .chain([Source::Spdif(false), Source::Spdif(true)]);
    all.filter_map(|s| s.to_wire().map(|w| (w, s.to_string())))
        .collect()
}

/// Every destination the patchbay can offer.
pub fn available_destinations() -> Vec<(u8, String)> {
    let all = [
        Destination::MainOut(false),
        Destination::MainOut(true),
        Destination::Phones(false),
        Destination::Phones(true),
        Destination::Es5(false),
        Destination::Es5(true),
    ]
    .into_iter()
    .chain((1..=8).map(Destination::Output))
    .chain((1..=16).map(Destination::Bus));
    all.filter_map(|d| d.to_wire().map(|w| (w, d.to_string())))
        .collect()
}

/// The stereo link families and pairs a UI can offer, with their current state.
pub fn links(state: &DeviceState) -> Vec<(Family, u8, String, bool)> {
    let families = [
        (Family::Input, "Input"),
        (Family::Bus, "Bus"),
        (Family::UsbLow, "USB"),
        (Family::UsbHigh, "USB"),
        (Family::Mix, "MIX"),
        (Family::SpdifOrMix2, "MIX 2"),
    ];
    let mut out = Vec::new();
    for (family, name) in families {
        for pair in 0..family.pair_count() {
            let even = (pair * 2) as u8;
            let offset = if family == Family::UsbHigh { 8 } else { 0 };
            out.push((
                family,
                even,
                format!("{name} {}/{}", even + 1 + offset, even + 2 + offset),
                state.config.links.is_linked(family, even),
            ));
        }
    }
    out
}
