//! Input DC-blocking filter pairs.

use es9_protocol::dcblock::{self, PAIR_COUNT, PAIRS};
use es9_protocol::tables::input_to_wire;

/// The claim that makes the odd-looking pairing believable.
///
/// A filter covers two adjacent ADC channels. Input numbers are permuted on the wire, so
/// if the pairing is really about ADC channels then every pair must map to two wire bytes
/// that differ only in the low bit — and "inputs 3/8", which looks like a typo in the
/// official tool, must come out adjacent like all the others.
#[test]
fn every_pair_is_two_adjacent_wire_channels() {
    for (bit, &(a, b)) in PAIRS.iter().enumerate() {
        let wa = input_to_wire(a).expect("input a has a wire byte");
        let wb = input_to_wire(b).expect("input b has a wire byte");
        let (low, high) = if wa < wb { (wa, wb) } else { (wb, wa) };
        assert_eq!(
            high,
            low + 1,
            "bit {bit} covers inputs {a}/{b}, wire {wa:#04X}/{wb:#04X}, which are not adjacent"
        );
        assert_eq!(
            low % 2,
            0,
            "bit {bit}: pair {a}/{b} should start on an even wire channel, got {low:#04X}"
        );
    }
}

#[test]
fn all_fourteen_inputs_are_covered_exactly_once() {
    let mut seen = Vec::new();
    for &(a, b) in &PAIRS {
        seen.push(a);
        seen.push(b);
    }
    seen.sort_unstable();
    let expected: Vec<u8> = (1..=14).collect();
    assert_eq!(seen, expected);
}

#[test]
fn input_three_and_eight_share_a_filter() {
    // The pairing users are most likely to get wrong, so it is asserted by name.
    assert_eq!(dcblock::bit_for_input(3), dcblock::bit_for_input(8));
    assert_eq!(dcblock::bit_for_input(3), Some(1));
    assert_eq!(dcblock::label(1).as_deref(), Some("Inputs 3/8"));
}

#[test]
fn adjacent_inputs_do_not_all_share_a_filter() {
    // Guards against anyone "tidying" the table into sequential pairs.
    assert_ne!(dcblock::bit_for_input(3), dcblock::bit_for_input(4));
    assert_ne!(dcblock::bit_for_input(7), dcblock::bit_for_input(8));
}

#[test]
fn setting_and_clearing_one_bit_leaves_the_others_alone() {
    let mut bits = 0u8;
    for bit in 0..PAIR_COUNT as u8 {
        bits = dcblock::with(bits, bit, true);
    }
    assert_eq!(bits, 0x7F, "all seven pairs on is a seven-bit mask");

    let cleared = dcblock::with(bits, 3, false);
    assert!(!dcblock::is_on(cleared, 3));
    for bit in (0..PAIR_COUNT as u8).filter(|b| *b != 3) {
        assert!(
            dcblock::is_on(cleared, bit),
            "bit {bit} should be untouched"
        );
    }
}

#[test]
fn an_out_of_range_bit_is_ignored() {
    // The payload field is seven bits; an eighth would be a value the module never sends.
    assert_eq!(dcblock::with(0x7F, 7, true), 0x7F);
    assert_eq!(dcblock::with(0x00, 7, true), 0x00);
    assert!(!dcblock::is_on(0xFF, 7));
    assert_eq!(dcblock::pair(7), None);
    assert_eq!(dcblock::label(7), None);
}

#[test]
fn every_input_maps_back_to_its_pair() {
    for input in 1..=14u8 {
        let bit = dcblock::bit_for_input(input).expect("every input has a filter");
        let (a, b) = dcblock::pair(bit).expect("every bit has a pair");
        assert!(
            a == input || b == input,
            "input {input} not in its own pair"
        );
    }
    assert_eq!(dcblock::bit_for_input(0), None);
    assert_eq!(dcblock::bit_for_input(15), None);
}
