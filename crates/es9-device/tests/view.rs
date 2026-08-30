//! Presentation model tests.
//!
//! These pin the labelling and stereo-collapsing behaviour that the UI depends on, so a
//! regression shows up here rather than as a mislabelled fader mid-set.

use es9_device::action::Action;
use es9_device::view;
use es9_device::{DeviceState, Session};
use es9_protocol::config::Options;
use es9_protocol::tables::{Destination, Family, Source};

fn wired() -> Session {
    let mut s = Session::default();
    // Mixer block 2 takes USB 1-8; its outputs go to the main outs and phones.
    for ch in 0..8u8 {
        s.state.config.capture[2][usize::from(ch)] =
            Source::Usb(ch + 1).to_wire().expect("valid source");
    }
    let outs = [
        Destination::MainOut(false),
        Destination::MainOut(true),
        Destination::Phones(false),
        Destination::Phones(true),
        Destination::Output(1),
        Destination::Output(2),
        Destination::Output(3),
        Destination::Output(4),
    ];
    for (ch, d) in outs.iter().enumerate() {
        s.state.config.outputs[2][ch] = d.to_wire().expect("valid destination");
    }
    s
}

#[test]
fn mono_mixes_list_every_channel_separately() {
    let s = wired();
    let mixes = view::mixer(&s.state);
    assert_eq!(mixes.len(), 8, "eight mono mixes with mixer 2 off");

    let first = &mixes[0];
    assert_eq!(first.name, "Mix 1");
    assert_eq!(first.destination, "Main Out L");
    assert!(!first.stereo);
    assert_eq!(first.strips.len(), 8);
    assert_eq!(first.strips[0].label, "USB 1");
    assert_eq!(first.strips[0].level_cc, 0);
    assert_eq!(first.strips[7].level_cc, 7);
    assert!(first.strips[0].pan.is_none(), "mono mixes have no pan");
}

#[test]
fn a_stereo_mix_pair_becomes_one_mix_with_pan() {
    let mut s = wired();
    s.dispatch(Action::SetLink {
        family: Family::Mix,
        even_index: 0,
        on: true,
    });
    let mixes = view::mixer(&s.state);

    assert_eq!(mixes.len(), 7, "mixes 1 and 2 merged into one");
    let first = &mixes[0];
    assert_eq!(first.name, "Mix 1/2");
    assert_eq!(first.destination, "Main Out L/R", "labels merge compactly");
    assert!(first.stereo);
    assert_eq!(first.strips[0].pan_cc, Some(8));
    assert_eq!(first.strips[0].level_cc, 0);
}

#[test]
fn a_stereo_source_becomes_one_strip_labelled_as_a_pair() {
    let mut s = wired();
    s.dispatch(Action::SetLink {
        family: Family::UsbLow,
        even_index: 0,
        on: true,
    });
    let mixes = view::mixer(&s.state);
    let first = &mixes[0];

    assert_eq!(first.strips.len(), 7, "channels 1 and 2 collapsed");
    assert_eq!(first.strips[0].label, "USB 1/2");
    assert!(first.strips[0].stereo);
    assert_eq!(first.strips[1].label, "USB 3", "the rest are untouched");
}

#[test]
fn mixer_2_adds_the_upper_mixes() {
    let mut s = wired();
    s.dispatch(Action::SetOptions(Options {
        mixer2: true,
        midi_thru: false,
    }));
    let mixes = view::mixer(&s.state);
    assert_eq!(mixes.len(), 16);
    assert_eq!(mixes[8].name, "Mix 9");
    assert_eq!(mixes[8].strips[0].level_cc, 64);
}

#[test]
fn levels_are_shown_in_decibels_with_silence_distinguished() {
    let mut s = wired();
    s.dispatch(Action::SetMacro { cc: 0, value: 103 });
    let mixes = view::mixer(&s.state);
    let strip = &mixes[0].strips[0];
    assert_eq!(strip.level, 103);
    let db = strip.level_db.expect("audible");
    assert!(db.abs() < 0.01, "103 is unity, got {db}");

    s.dispatch(Action::SetMacro { cc: 0, value: 0 });
    let mixes = view::mixer(&s.state);
    assert_eq!(
        mixes[0].strips[0].level_db, None,
        "silence is not a dB value"
    );
}

#[test]
fn the_cc_table_covers_all_128_and_marks_shadowed_entries() {
    let mut s = wired();
    let rows = view::cc_rows(&s.state);
    assert_eq!(rows.len(), 128);
    assert_eq!(rows[0].target.as_deref(), Some("Mix 1, ch 1, level"));
    assert!(rows[64].target.is_none(), "mixer 2 is off");

    s.dispatch(Action::SetLink {
        family: Family::Mix,
        even_index: 0,
        on: true,
    });
    let rows = view::cc_rows(&s.state);
    assert_eq!(rows[8].target.as_deref(), Some("Mix 1/2, ch 1, pan"));
    assert_eq!(rows[0].target.as_deref(), Some("Mix 1/2, ch 1, level"));
}

#[test]
fn routing_shows_all_sixty_four_assignments() {
    let s = wired();
    let r = view::routing(&s.state);
    assert_eq!(r.capture.len(), 32);
    assert_eq!(r.outputs.len(), 32);

    let mixer_in = r
        .capture
        .iter()
        .find(|c| c.block == 2 && c.channel == 0)
        .expect("present");
    assert_eq!(mixer_in.label, "USB 1");
    assert!(mixer_in.known);
}

#[test]
fn unrecognised_wire_values_are_shown_rather_than_hidden() {
    let mut s = DeviceState::default();
    s.config.capture[0][0] = 0x4F; // not a valid source family
    let r = view::routing(&s);
    let ch = &r.capture[0];
    assert!(!ch.known);
    assert_eq!(ch.label, "? 0x4F");
    assert_eq!(ch.wire, 0x4F, "preserved so a save does not corrupt it");
}

#[test]
fn the_patchbay_offers_inputs_in_natural_order() {
    let sources = view::available_sources();
    assert_eq!(sources[0].1, "Input 1");
    assert_eq!(sources[13].1, "Input 14");
    // Input 1 is wire byte 0x78, not 0x70 -- the permutation stays hidden.
    assert_eq!(sources[0].0, 0x78);

    let destinations = view::available_destinations();
    assert!(
        destinations
            .iter()
            .any(|(w, n)| *n == "Output 7" && *w == 11)
    );
}

#[test]
fn link_list_labels_the_upper_usb_bank_correctly() {
    let s = wired();
    let links = view::links(&s.state);
    assert!(links.iter().any(|(_, _, name, _)| name == "USB 1/2"));
    assert!(
        links.iter().any(|(_, _, name, _)| name == "USB 9/10"),
        "the second USB bank must not be labelled 1/2 again"
    );
    assert_eq!(
        links.iter().filter(|(f, ..)| *f == Family::Input).count(),
        7,
        "14 inputs give 7 pairs"
    );
}

#[test]
fn status_reports_dirtiness_and_the_slot_being_edited() {
    let mut s = wired();
    assert!(!view::status(&s.state).dirty);
    s.dispatch(Action::SetHpf(1));
    let st = view::status(&s.state);
    assert!(st.dirty);
    assert_eq!(st.editing, es9_device::EditTarget::Hosted);
}

#[test]
fn dc_filters_cover_every_input_and_flag_the_odd_pair() {
    let state = DeviceState::default();
    let filters = view::dc_filters(&state);
    assert_eq!(filters.len(), 7);

    let mut inputs: Vec<u8> = filters
        .iter()
        .flat_map(|f| [f.inputs.0, f.inputs.1])
        .collect();
    inputs.sort_unstable();
    assert_eq!(inputs, (1..=14).collect::<Vec<u8>>());

    // Only inputs 3/8 are non-consecutive, and the view must say so.
    let surprising: Vec<&str> = filters
        .iter()
        .filter(|f| f.surprising)
        .map(|f| f.label.as_str())
        .collect();
    assert_eq!(surprising, ["Inputs 3/8"]);
}

#[test]
fn dc_filter_state_follows_the_bitmap() {
    let mut state = DeviceState::default();
    state.config.hpf = 0b000_0010; // bit 1 only: inputs 3/8
    let filters = view::dc_filters(&state);
    let on: Vec<&str> = filters
        .iter()
        .filter(|f| f.on)
        .map(|f| f.label.as_str())
        .collect();
    assert_eq!(on, ["Inputs 3/8"]);
}

#[test]
fn a_resting_pan_reads_as_centre_not_hard_left() {
    // The module stores pan in the aux byte of the *level* cell, one less than the value
    // written (checklist H3). Reading the pan CC's own value gives zero, which renders
    // every strip hard left — which is exactly what the app did before this was measured.
    let mut state = DeviceState {
        config: es9_device::presets::standalone(),
        ..Default::default()
    };
    // Link mixes 1/2 so the strips have pans at all.
    state
        .config
        .links
        .set(es9_protocol::tables::Family::Mix, 0, true);

    let level_cc = view::mixer(&state)[0].strips[0].level_cc;
    state.macro_aux[usize::from(level_cc)] = es9_protocol::scales::PAN_CENTRE_STORED;

    let strip = &view::mixer(&state)[0].strips[0];
    assert!(strip.pan_cc.is_some(), "a stereo mix has a pan");
    assert_eq!(strip.pan, Some(0), "a resting pan is centre");
}

#[test]
fn pan_follows_the_aux_byte_of_its_level_cell() {
    let mut state = DeviceState {
        config: es9_device::presets::standalone(),
        ..Default::default()
    };
    state
        .config
        .links
        .set(es9_protocol::tables::Family::Mix, 0, true);

    let strip = view::mixer(&state)[0].strips[0].clone();
    let level_cc = usize::from(strip.level_cc);
    let pan_cc = usize::from(strip.pan_cc.expect("stereo mix has a pan"));

    // Writing to the pan CC's own value must not move the displayed pan.
    state.macro_values[pan_cc] = 127;
    assert_eq!(view::mixer(&state)[0].strips[0].pan, Some(-63));

    // The aux byte of the level cell is what counts.
    state.macro_aux[level_cc] = es9_protocol::scales::PAN_CENTRE_STORED + 20;
    assert_eq!(view::mixer(&state)[0].strips[0].pan, Some(20));
    state.macro_aux[level_cc] = es9_protocol::scales::PAN_CENTRE_STORED - 20;
    assert_eq!(view::mixer(&state)[0].strips[0].pan, Some(-20));
}

#[test]
fn dc_offsets_cover_the_eight_analogue_outputs() {
    let state = DeviceState {
        config: es9_device::presets::standalone(),
        ..Default::default()
    };
    let offsets = view::dc_offsets(&state);
    assert_eq!(offsets.len(), 8);
    assert_eq!(offsets[0].label, "Output 1");
    assert_eq!(offsets[7].label, "Output 8");
}

#[test]
fn an_offset_is_only_effective_where_a_mixer_feeds_the_output() {
    // The offset is implemented with the mixer hardware, so an output fed from USB
    // playback silently ignores it. In the standalone preset, mixes 5-8 are routed to
    // Outputs 1-4, so those four take an offset and the rest do not.
    let state = DeviceState {
        config: es9_device::presets::standalone(),
        ..Default::default()
    };
    let offsets = view::dc_offsets(&state);

    let effective: Vec<&str> = offsets
        .iter()
        .filter(|o| o.effective)
        .map(|o| o.label.as_str())
        .collect();
    assert_eq!(effective, ["Output 1", "Output 2", "Output 3", "Output 4"]);

    assert_eq!(offsets[0].fed_by.as_deref(), Some("Mix 5"));
    assert_eq!(
        offsets[4].fed_by, None,
        "Output 5 has no mixer routed to it"
    );
}

#[test]
fn routing_a_mix_to_an_output_makes_its_offset_effective() {
    let mut state = DeviceState {
        config: es9_device::presets::standalone(),
        ..Default::default()
    };
    assert!(!view::dc_offsets(&state)[4].effective);

    // Point mix 1 (block 2, channel 0) at Output 5.
    let wire = es9_protocol::tables::Destination::Output(5)
        .to_wire()
        .expect("Output 5 has a wire value");
    state.config.outputs[2][0] = wire;

    let offsets = view::dc_offsets(&state);
    assert!(
        offsets[4].effective,
        "Output 5 now has a mixer routed to it"
    );
    assert_eq!(offsets[4].fed_by.as_deref(), Some("Mix 1"));
}

#[test]
fn spdif_status_reports_both_directions_not_just_the_mode() {
    // Options bit 0 clear means block 3 *is* the S/PDIF processor. That alone says
    // nothing about whether either direction reaches anything, which is the whole point
    // of reporting more than the mode.
    let state = DeviceState {
        config: es9_device::presets::hosted(),
        ..Default::default()
    };
    let status = view::status(&state);
    assert!(!status.mixer2);

    let spdif = status.spdif.expect("block 3 is the S/PDIF processor");
    assert_eq!(
        spdif.output_from, "USB 5/6",
        "the S/PDIF output carries DAW channels 5-6, per the manual's default"
    );
    assert_eq!(
        spdif.input_to, None,
        "this preset points capture 15/16 at the mix, so the S/PDIF input reaches nothing"
    );
}

#[test]
fn capturing_the_spdif_input_shows_where_it_lands() {
    let mut state = DeviceState {
        config: es9_device::presets::hosted(),
        ..Default::default()
    };
    // Put the S/PDIF input back on capture 15/16, the module's own default.
    state.config.capture[1][6] = Source::Spdif(false).to_wire().expect("S/PDIF L");
    state.config.capture[1][7] = Source::Spdif(true).to_wire().expect("S/PDIF R");

    let spdif = view::status(&state)
        .spdif
        .expect("still the S/PDIF processor");
    assert_eq!(spdif.input_to.as_deref(), Some("capture 15/16"));
}

#[test]
fn enabling_mixer_2_leaves_no_spdif_to_report() {
    let state = DeviceState {
        config: es9_device::presets::standalone(),
        ..Default::default()
    };
    let status = view::status(&state);
    assert!(status.mixer2);
    assert_eq!(
        status.spdif, None,
        "block 3 is a mixer bank, so there is no S/PDIF processor to describe"
    );
}
