//! Transport tests. These cover the pure parts, so they run without a MIDI device.

use es9_midi::{Direction, Monitor};
use es9_midi::{PortInfo, SysexAssembler, find_es9};
use es9_protocol::encode;

#[test]
fn a_whole_message_in_one_callback_comes_straight_through() {
    let mut a = SysexAssembler::new();
    let msg = encode::request_config();
    assert_eq!(a.push(&msg), vec![msg]);
    assert_eq!(a.pending(), 0);
}

#[test]
fn a_fragmented_dump_is_reassembled() {
    // A configuration dump is 2313 bytes. Backends differ in whether they deliver a
    // message that size whole, so fragments must be handled.
    let dump = encode::config_dump(&es9_protocol::Config::default());
    assert_eq!(dump.len(), 2313);

    let mut a = SysexAssembler::new();
    let mut completed = Vec::new();
    for chunk in dump.chunks(64) {
        completed.extend(a.push(chunk));
    }
    assert_eq!(completed, vec![dump]);
}

#[test]
fn realtime_bytes_interleaved_mid_message_are_discarded() {
    // MIDI clock and active sensing may appear anywhere, including inside a SysEx
    // message. They must not end up in the payload.
    let msg = encode::set_raw_mix(3, 4, 0x1234);
    let mut noisy = Vec::new();
    for (i, b) in msg.iter().enumerate() {
        if i == 3 {
            noisy.push(0xF8); // timing clock
        }
        noisy.push(*b);
        if i == 6 {
            noisy.push(0xFE); // active sensing
        }
    }
    let mut a = SysexAssembler::new();
    assert_eq!(a.push(&noisy), vec![msg]);
}

#[test]
fn an_abandoned_message_does_not_corrupt_the_next_one() {
    let mut a = SysexAssembler::new();
    a.push(&[0xF0, 0x00, 0x21, 0x27]); // truncated, no F7
    assert!(a.pending() > 0);

    let good = encode::request_version();
    assert_eq!(a.push(&good), vec![good], "a new F0 restarts cleanly");
}

#[test]
fn channel_voice_traffic_between_messages_is_ignored() {
    let mut a = SysexAssembler::new();
    assert!(a.push(&[0x90, 0x40, 0x7F]).is_empty(), "note on");
    assert!(a.push(&[0xB0, 0x07, 0x64]).is_empty(), "control change");
    let msg = encode::request_mix();
    assert_eq!(a.push(&msg), vec![msg]);
}

#[test]
fn reset_drops_a_partial_message() {
    let mut a = SysexAssembler::new();
    a.push(&[0xF0, 0x00, 0x21]);
    a.reset();
    assert_eq!(a.pending(), 0);
    assert!(
        a.push(&[0x27, 0x19, 0xF7]).is_empty(),
        "no message in flight"
    );
}

#[test]
fn outgoing_messages_are_described_usefully() {
    let mut m = Monitor::default();
    m.record(Direction::Tx, &encode::set_macro(8, 64), 0);
    m.record(Direction::Tx, &encode::request_config(), 1);
    m.record(
        Direction::Tx,
        &encode::restore_from_flash(encode::FlashSlot::Standalone),
        2,
    );
    m.record(Direction::Tx, &encode::set_raw_mix(2, 0, 100), 3);

    let described: Vec<String> = m.entries().map(|e| e.describe()).collect();
    // The manual's own trick for finding a control's CC number: read the byte after 34.
    assert_eq!(described[0], "macro mix: CC 8 = 64");
    assert_eq!(described[1], "request configuration");
    assert_eq!(described[2], "restore from flash, standalone");
    assert_eq!(described[3], "set raw mix, mix 3");
}

#[test]
fn incoming_messages_are_decoded_for_the_log() {
    let mut m = Monitor::default();
    let dump = encode::config_dump(&es9_protocol::Config::default());
    m.record(Direction::Rx, &dump, 0);
    m.record(Direction::Rx, &encode::set_raw_mix(0, 7, 4096), 1);

    let described: Vec<String> = m.entries().map(|e| e.describe()).collect();
    assert_eq!(described[0], "configuration dump");
    assert_eq!(described[1], "raw mix 1/8 = 4096");
}

#[test]
fn undecodable_traffic_is_logged_rather_than_dropped() {
    let mut m = Monitor::default();
    m.record(
        Direction::Rx,
        &[0xF0, 0x00, 0x21, 0x27, 0x19, 0x08, 0xF7],
        0,
    );
    let entry = m.entries().next().expect("logged");
    assert!(
        entry.describe().starts_with("undecodable:"),
        "got {:?}",
        entry.describe()
    );
    assert_eq!(entry.hex(), "F0 00 21 27 19 08 F7");
}

#[test]
fn the_log_is_bounded_and_keeps_the_newest() {
    let mut m = Monitor::new(3);
    for i in 0..10u8 {
        m.record(Direction::Tx, &encode::set_macro(i, 0), u64::from(i));
    }
    assert_eq!(m.len(), 3);
    let first = m.entries().next().expect("entry");
    assert_eq!(first.describe(), "macro mix: CC 7 = 0", "oldest discarded");
}

#[test]
fn port_matching_is_tolerant_of_naming_variations() {
    let ports = |name: &str| {
        vec![
            PortInfo {
                index: 0,
                name: "Midi Through Port-0".to_string(),
            },
            PortInfo {
                index: 1,
                name: name.to_string(),
            },
        ]
    };
    for name in ["ES-9", "Expert Sleepers ES-9", "es9 MIDI 1", "ES_9"] {
        let p = ports(name);
        assert_eq!(
            find_es9(&p).map(|p| p.index),
            Some(1),
            "should match {name:?}"
        );
    }
    assert!(find_es9(&ports("Some Other Device")).is_none());
}
