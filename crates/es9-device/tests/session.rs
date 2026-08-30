//! End-to-end tests against the mock device.
//!
//! These are the real check on Phase 2: that the messages an action emits actually make
//! a device agree with the model, verified by reading the configuration back.

use es9_device::action::Action;
use es9_device::state::EditTarget;
use es9_device::throttle::Coalescer;
use es9_device::{DeviceState, MockEs9, Session};
use es9_protocol::config::Options;
use es9_protocol::tables::{Family, Source};

/// Feeds messages to the mock and folds its replies back into the model.
fn pump(mock: &mut MockEs9, state: &mut DeviceState, messages: Vec<Vec<u8>>) {
    for m in messages {
        for reply in mock.receive(&m) {
            if let Ok(incoming) = es9_protocol::decode(&reply) {
                state.ingest(incoming);
            }
        }
    }
}

fn connected() -> (Session, MockEs9) {
    let mut session = Session::default();
    let mut mock = MockEs9::default();
    pump(&mut mock, &mut session.state, Session::request_all());
    (session, mock)
}

#[test]
fn connecting_populates_identity_and_configuration() {
    let (session, _mock) = connected();
    assert_eq!(
        session.state.identity.version.as_deref(),
        Some("ES-9 1.3.1")
    );
    assert_eq!(session.state.identity.sample_rate, Some(48_000));
    assert!(!session.state.dirty, "a fresh load is not dirty");
}

#[test]
fn the_device_ends_up_agreeing_with_the_model() {
    let (mut session, mut mock) = connected();

    let actions = vec![
        Action::SetRawMix {
            mix: 3,
            channel: 5,
            value: 0x1ABC,
        },
        Action::SetOptions(Options {
            mixer2: true,
            midi_thru: true,
        }),
        Action::SetMidiChannels { usb: 9, din: 14 },
        Action::SetSmoothing { mix: 11, on: true },
        Action::SetHpf(0x2A),
        Action::SetDcOffset {
            channel: 6,
            offset: -1234,
        },
        Action::SetOutput {
            block: 2,
            channel: 4,
            destination: 12,
        },
        Action::SetLink {
            family: Family::Mix,
            even_index: 0,
            on: true,
        },
    ];
    for action in actions {
        let applied = session.dispatch(action);
        pump(&mut mock, &mut session.state, applied.messages);
    }

    // What the model believes, before asking the device.
    let expected = session.state.config.clone();

    // Now read the device's own view back and confirm it matches.
    pump(
        &mut mock,
        &mut session.state,
        vec![es9_protocol::encode::request_config()],
    );
    assert_eq!(session.state.config, expected);
}

#[test]
fn macro_values_round_trip_through_a_mix_dump() {
    let (mut session, mut mock) = connected();
    let applied = session.dispatch(Action::SetMacro { cc: 17, value: 103 });
    pump(&mut mock, &mut session.state, applied.messages);

    session.state.macro_values[17] = 0; // forget it locally
    pump(
        &mut mock,
        &mut session.state,
        vec![es9_protocol::encode::request_mix()],
    );
    assert_eq!(session.state.macro_values[17], 103);
}

#[test]
fn undo_restores_both_model_and_device() {
    let (mut session, mut mock) = connected();
    let before = session.state.config.clone();

    let applied = session.dispatch(Action::SetRawMix {
        mix: 2,
        channel: 1,
        value: 0x4000,
    });
    pump(&mut mock, &mut session.state, applied.messages);
    assert_ne!(session.state.config, before);

    let undone = session.undo().expect("something to undo");
    pump(&mut mock, &mut session.state, undone.messages);
    assert_eq!(session.state.config, before, "model restored");

    // And confirm the device agrees rather than just the model.
    pump(
        &mut mock,
        &mut session.state,
        vec![es9_protocol::encode::request_config()],
    );
    assert_eq!(session.state.config, before, "device restored");
}

#[test]
fn redo_reapplies_and_a_new_action_clears_the_redo_history() {
    let (mut session, _mock) = connected();
    session.dispatch(Action::SetHpf(0x7F));
    session.undo().expect("undo");
    assert!(session.can_redo());

    session.redo().expect("redo");
    assert_eq!(session.state.config.hpf, 0x7F);
    assert!(!session.can_redo());

    session.undo().expect("undo again");
    session.dispatch(Action::SetHpf(0x01));
    assert!(!session.can_redo(), "a new action discards redo history");
}

#[test]
fn a_batch_undoes_as_one_step_and_in_reverse_order() {
    let (mut session, _mock) = connected();
    let before = session.state.config.clone();

    session.dispatch(Action::Batch(vec![
        Action::SetHpf(0x11),
        Action::SetSmoothing { mix: 0, on: true },
        Action::SetMidiChannels { usb: 3, din: 4 },
    ]));
    assert_ne!(session.state.config, before);

    session.undo().expect("one undo for the whole batch");
    assert_eq!(session.state.config, before);
    assert!(!session.can_undo(), "the batch was a single step");
}

#[test]
fn editing_the_standalone_slot_loads_it_first() {
    // The trap this guards against: connecting over USB forces hosted mode, so editing
    // the standalone config without restoring it first silently edits hosted state.
    let (mut session, mut mock) = connected();

    // Give the two slots different contents so a mix-up would be visible.
    mock.flash[0].hpf = 0x55; // standalone
    mock.flash[1].hpf = 0x2A; // hosted

    let messages = session.begin_edit(EditTarget::Standalone);
    let first = es9_protocol::frame::parse(&messages[0]).expect("valid frame");
    assert_eq!(first.cmd, 0x25, "restore from flash must come first");
    assert_eq!(first.payload[0], 0, "slot 0 is standalone");

    pump(&mut mock, &mut session.state, messages);
    assert_eq!(
        session.state.config.hpf, 0x55,
        "we are looking at the standalone configuration"
    );
    assert_eq!(session.state.editing, EditTarget::Standalone);
    assert!(!session.state.dirty);
}

#[test]
fn saving_writes_back_to_the_slot_being_edited() {
    let (mut session, mut mock) = connected();
    let begin = session.begin_edit(EditTarget::Standalone);
    pump(&mut mock, &mut session.state, begin);

    let applied = session.dispatch(Action::SetHpf(0x33));
    pump(&mut mock, &mut session.state, applied.messages);
    assert!(session.state.dirty);

    let save = session.save();
    pump(&mut mock, &mut session.state, vec![save]);
    assert!(!session.state.dirty);
    assert_eq!(mock.flash[0].hpf, 0x33, "standalone slot updated");
    assert_ne!(mock.flash[1].hpf, 0x33, "hosted slot untouched");
    assert_eq!(session.state.identity.last_message.as_deref(), Some("OK"));
}

#[test]
fn linking_a_mix_pair_reports_every_cc_it_rewrote() {
    let (mut session, _mock) = connected();
    let applied = session.dispatch(Action::SetLink {
        family: Family::Mix,
        even_index: 0,
        on: true,
    });
    assert_eq!(
        applied.cc_changes.len(),
        16,
        "both rows of the pair change meaning"
    );
    assert!(applied.cc_changes.iter().any(|c| c.cc == 8));
}

#[test]
fn ordinary_actions_report_no_cc_changes() {
    let (mut session, _mock) = connected();
    let applied = session.dispatch(Action::SetRawMix {
        mix: 0,
        channel: 0,
        value: 1234,
    });
    assert!(applied.cc_changes.is_empty());
}

#[test]
fn assigning_a_linked_source_brings_its_partner_along() {
    let (mut session, mut mock) = connected();
    let link = session
        .dispatch(Action::SetLink {
            family: Family::UsbLow,
            even_index: 2,
            on: true,
        })
        .messages;
    pump(&mut mock, &mut session.state, link);

    // Point mixer channel 0 at USB 3, which is the left half of a linked pair.
    let applied = session.dispatch(Action::SetCapture {
        block: 2,
        channel: 0,
        source: Source::Usb(3).to_wire().expect("valid"),
    });
    pump(&mut mock, &mut session.state, applied.messages);

    assert_eq!(
        session.state.config.capture_source(2, 0),
        Some(Source::Usb(3))
    );
    assert_eq!(
        session.state.config.capture_source(2, 1),
        Some(Source::Usb(4)),
        "the partner channel follows so the stereo signal stays intact"
    );
}

#[test]
fn unlinked_sources_do_not_disturb_their_neighbour() {
    let (mut session, _mock) = connected();
    let original = session.state.config.capture[2][1];
    session.dispatch(Action::SetCapture {
        block: 2,
        channel: 0,
        source: Source::Usb(3).to_wire().expect("valid"),
    });
    assert_eq!(session.state.config.capture[2][1], original);
}

#[test]
fn a_config_upload_reassembles_on_the_device() {
    let (mut session, mut mock) = connected();
    let mut wanted = session.state.config.clone();
    wanted.hpf = 0x7F;
    wanted.midi_usb = 11;
    wanted.raw_mix[9][3] = 0x2222;

    let chunks = es9_protocol::encode::config_upload(&wanted);
    assert_eq!(chunks.len(), 24);
    for chunk in &chunks {
        mock.receive(chunk);
    }
    assert_eq!(mock.config, wanted, "all 24 chunks reassembled");

    // Read it back, as the app must since the upload is unacknowledged.
    pump(
        &mut mock,
        &mut session.state,
        vec![es9_protocol::encode::request_config()],
    );
    assert_eq!(session.state.config, wanted);
}

#[test]
fn an_incomplete_upload_leaves_the_device_untouched() {
    let mut mock = MockEs9::default();
    let before = mock.config.clone();
    let mut wanted = before.clone();
    wanted.hpf = 0x7F;

    for chunk in es9_protocol::encode::config_upload(&wanted).iter().take(23) {
        mock.receive(chunk);
    }
    assert_eq!(mock.config, before, "nothing applies until the last chunk");
}

#[test]
fn echoed_writes_are_absorbed_without_looping() {
    // The hardware's echo behaviour is unverified, so a client must survive either way.
    let mut mock = MockEs9::default();
    mock.echo_writes = true;
    let mut session = Session::default();

    let applied = session.dispatch(Action::SetRawMix {
        mix: 1,
        channel: 2,
        value: 999,
    });
    let mut replies = Vec::new();
    for m in &applied.messages {
        replies.extend(mock.receive(m));
    }
    assert_eq!(replies.len(), 1, "the module echoed the write");

    for reply in replies {
        session
            .state
            .ingest(es9_protocol::decode(&reply).expect("decodes"));
    }
    assert_eq!(session.state.config.raw_mix[1][2], 999);
    assert!(!session.can_redo());
}

#[test]
fn coalescer_sends_one_value_per_interval_not_one_per_event() {
    let mut c: Coalescer<u8, u8> = Coalescer::new(20);

    // A drag: many values on one control inside a single interval.
    for v in 0..50u8 {
        c.push(7, v);
    }
    assert!(c.flush(10).is_empty(), "too soon to send");

    let out = c.flush(30);
    assert_eq!(out, vec![(7, 49)], "only the latest value survives");
    assert!(c.is_empty());
}

#[test]
fn coalescer_keeps_controls_separate_and_flushes_the_tail() {
    let mut c: Coalescer<u8, u8> = Coalescer::new(20);
    c.push(1, 10);
    c.push(2, 20);
    c.push(1, 11);
    assert_eq!(c.flush(100), vec![(1, 11), (2, 20)]);

    // A drag that ends mid-interval must not strand its final value.
    c.push(3, 42);
    assert!(c.flush(105).is_empty());
    assert_eq!(c.flush_now(105), vec![(3, 42)]);
}

#[test]
fn a_dc_filter_toggle_reaches_the_device_and_undoes_on_its_own() {
    let (mut session, mut mock) = connected();
    // Start with every filter on, the sensible all-audio state.
    let applied = session.dispatch(Action::SetHpf(0x7F));
    pump(&mut mock, &mut session.state, applied.messages);

    // Turn off the filter for inputs 3/8, as you would to patch a CV into input 8.
    let applied = session.dispatch(Action::SetDcBlock { bit: 1, on: false });
    pump(&mut mock, &mut session.state, applied.messages);
    assert_eq!(session.state.config.hpf as u8, 0b111_1101);

    // Confirm the module agrees rather than only the model.
    pump(
        &mut mock,
        &mut session.state,
        vec![es9_protocol::encode::request_config()],
    );
    assert_eq!(
        session.state.config.hpf as u8, 0b111_1101,
        "the device kept the cleared bit"
    );

    let undone = session.undo().expect("something to undo");
    pump(&mut mock, &mut session.state, undone.messages);
    assert_eq!(
        session.state.config.hpf as u8, 0x7F,
        "undo restores just that filter"
    );
}

#[test]
fn toggling_one_dc_filter_leaves_the_others_alone() {
    let (mut session, mut mock) = connected();
    let applied = session.dispatch(Action::SetHpf(0));
    pump(&mut mock, &mut session.state, applied.messages);

    for bit in [0u8, 3, 6] {
        let applied = session.dispatch(Action::SetDcBlock { bit, on: true });
        pump(&mut mock, &mut session.state, applied.messages);
    }
    assert_eq!(session.state.config.hpf as u8, 0b100_1001);

    let applied = session.dispatch(Action::SetDcBlock { bit: 3, on: false });
    pump(&mut mock, &mut session.state, applied.messages);
    assert_eq!(session.state.config.hpf as u8, 0b100_0001);
}

#[test]
fn loading_a_configuration_replaces_everything_and_undoes_as_one_step() {
    let (mut session, mut mock) = connected();
    let before = session.state.config.clone();

    // The hosted preset differs from the standalone one the mock boots with, so this is
    // a genuine whole-configuration change rather than a no-op.
    let target = es9_device::mixer_defaults::hosted();
    assert_ne!(target, before, "the two presets must actually differ");

    let applied = session.dispatch(Action::LoadConfig(Box::new(target.clone())));
    pump(&mut mock, &mut session.state, applied.messages);
    assert_eq!(session.state.config, target, "model took the preset");

    // The module agrees, not just the model.
    pump(
        &mut mock,
        &mut session.state,
        vec![es9_protocol::encode::request_config()],
    );
    assert_eq!(session.state.config, target, "device took the preset");

    let undone = session.undo().expect("something to undo");
    pump(&mut mock, &mut session.state, undone.messages);
    pump(
        &mut mock,
        &mut session.state,
        vec![es9_protocol::encode::request_config()],
    );
    assert_eq!(session.state.config, before, "one undo restores everything");
}

#[test]
fn a_recorded_undo_reverses_something_the_module_did_itself() {
    // A factory reset happens inside the module, so there is no action to invert. The
    // caller captures the configuration first and records the restore explicitly.
    let (mut session, mut mock) = connected();
    let before = session.state.config.clone();

    // Stand in for the module having reset itself.
    session.state.config = es9_device::mixer_defaults::hosted();
    session.record_undo(Action::LoadConfig(Box::new(before.clone())));
    assert!(session.can_undo());

    let undone = session.undo().expect("something to undo");
    pump(&mut mock, &mut session.state, undone.messages);
    assert_eq!(session.state.config, before);
}
