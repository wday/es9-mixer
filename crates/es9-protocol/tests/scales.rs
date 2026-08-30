//! Gain scale conversions.

use es9_protocol::scales::{
    MACRO_UNITY, PAN_CENTRE, PAN_CENTRE_STORED, RAW_MAX, RAW_UNITY, db_to_macro, db_to_raw,
    macro_to_db, macro_to_raw, pan_to_macro, raw_to_db, stored_to_pan, written_to_stored,
};
use es9_protocol::tables::{Destination, INPUT_CAPTURE_LOOKUP, Source};

#[test]
fn raw_gain_anchors_match_the_hardware() {
    assert_eq!(raw_to_db(0), f32::NEG_INFINITY);
    assert!((raw_to_db(RAW_UNITY) - 0.0).abs() < 1e-4, "0x2000 is unity");
    assert!((raw_to_db(RAW_MAX) - 12.0).abs() < 1e-4, "0x7FFF is +12 dB");
    assert!((raw_to_db(4096) + 6.0).abs() < 1e-4, "halving is -6 dB");
}

#[test]
fn raw_gain_round_trips_through_decibels() {
    for v in [1u16, 100, 1000, RAW_UNITY, 20000, RAW_MAX] {
        let back = db_to_raw(raw_to_db(v));
        let drift = i32::from(back) - i32::from(v);
        assert!(drift.abs() <= 1, "{v} came back as {back}");
    }
}

#[test]
fn macro_scale_is_piecewise_with_fine_resolution_above_minus_24() {
    assert_eq!(macro_to_db(0), f32::NEG_INFINITY);
    assert!(
        (macro_to_db(55) + 24.0).abs() < 1e-4,
        "the knee sits at -24 dB"
    );
    assert!((macro_to_db(MACRO_UNITY) - 0.0).abs() < 1e-4, "103 is 0 dB");
    assert!((macro_to_db(127) - 12.0).abs() < 1e-4, "127 is +12 dB");

    // Half a decibel per step above the knee.
    assert!((macro_to_db(57) - macro_to_db(55) - 1.0).abs() < 1e-4);
}

#[test]
fn macro_level_round_trips_within_its_quantisation() {
    for v in 1..=127u8 {
        assert_eq!(db_to_macro(macro_to_db(v)), v, "level {v}");
    }
}

#[test]
fn silence_maps_to_zero_on_both_scales() {
    assert_eq!(db_to_raw(-80.0), 0);
    assert_eq!(db_to_macro(-80.0), 0);
}

#[test]
fn input_permutation_round_trips_and_is_hidden_from_callers() {
    for n in 1..=14u8 {
        let wire = Source::Input(n).to_wire().expect("valid input");
        assert_eq!(wire, INPUT_CAPTURE_LOOKUP[usize::from(n - 1)]);
        assert_eq!(Source::from_wire(wire), Some(Source::Input(n)));
    }
    // Input 1 is not wire byte 0x70 -- the permutation is real, not decorative.
    assert_eq!(Source::Input(1).to_wire(), Some(0x78));
}

#[test]
fn every_source_round_trips_through_the_wire() {
    let all = (1..=16u8)
        .map(Source::Usb)
        .chain((1..=8).map(Source::Mix))
        .chain([Source::Spdif(false), Source::Spdif(true)])
        .chain((1..=16).map(Source::Bus))
        .chain((1..=14).map(Source::Input));
    for s in all {
        let wire = s.to_wire().unwrap_or_else(|| panic!("{s:?} should encode"));
        assert_eq!(Source::from_wire(wire), Some(s), "{s:?}");
    }
}

#[test]
fn every_destination_round_trips_including_the_swapped_outputs() {
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
    for d in all {
        let wire = d.to_wire().unwrap_or_else(|| panic!("{d:?} should encode"));
        assert_eq!(Destination::from_wire(wire), Some(d), "{d:?}");
    }
    // Outputs 7 and 8 are deliberately swapped relative to each other on the wire.
    assert_eq!(Destination::Output(7).to_wire(), Some(11));
    assert_eq!(Destination::Output(8).to_wire(), Some(10));
}

#[test]
fn macro_unity_converts_to_the_raw_value_the_hardware_uses() {
    // Measured on a real ES-9 on 2026-08-30: macro 103 is unity and the module holds
    // 0x2000 in the raw matrix for it. This is the anchor point of the whole curve, so
    // it is asserted exactly.
    assert_eq!(macro_to_raw(MACRO_UNITY), 0x2000);
    assert_eq!(macro_to_db(MACRO_UNITY), 0.0);
}

#[test]
fn the_macro_to_raw_curve_is_close_to_hardware_but_not_exact() {
    // Also measured: macro 100 settled at 6832. The documented curves give 6889. The gap
    // is about 0.07 dB. Asserted as a bound rather than an equality so that the known
    // deviation is recorded and a *large* future divergence still fails the test.
    let computed = f32::from(macro_to_raw(100));
    let measured = 6832.0_f32;
    let error_db = 20.0 * (computed / measured).log10();
    assert!(
        error_db.abs() < 0.1,
        "macro 100 computed {computed}, hardware measured {measured}, {error_db:.3} dB apart"
    );
    assert_ne!(
        macro_to_raw(100),
        6832,
        "if this now matches exactly, the curve was corrected — update Q9"
    );
}

#[test]
fn writing_pan_centre_stores_one_less() {
    // Measured on hardware: writing 64 to the pan CC leaves 63 in the level cell's aux.
    // Write bias and read bias differ by one because the module subtracts one.
    assert_eq!(pan_to_macro(0), PAN_CENTRE);
    assert_eq!(written_to_stored(PAN_CENTRE), PAN_CENTRE_STORED);
    assert_eq!(stored_to_pan(PAN_CENTRE_STORED), 0);
}

#[test]
fn pan_round_trips_through_the_module_off_by_one() {
    // The full loop as the hardware performs it: write, the module stores one less, read.
    for pan in -62..=63i8 {
        let written = pan_to_macro(pan);
        let stored = written_to_stored(written);
        assert_eq!(stored_to_pan(stored), pan, "pan {pan} wrote {written}");
    }
}

#[test]
fn the_measured_sweep_is_reproduced_exactly() {
    // Straight from the hardware sweep on 2026-08-30. Writing 0 is the one value that
    // does not lose a step, because the stored byte saturates rather than wrapping.
    for (written, stored) in [
        (0u8, 0u8),
        (1, 0),
        (32, 31),
        (63, 62),
        (64, 63),
        (65, 64),
        (96, 95),
        (126, 125),
        (127, 126),
    ] {
        assert_eq!(written_to_stored(written), stored, "writing {written}");
    }
}

#[test]
fn pan_extremes_and_out_of_range_values_are_clamped() {
    assert_eq!(pan_to_macro(-63), 1);
    assert_eq!(pan_to_macro(63), 127);
    assert_eq!(stored_to_pan(0), -63);
    assert_eq!(stored_to_pan(127), 63);
    assert_eq!(pan_to_macro(-100), 1);
    assert_eq!(pan_to_macro(100), 127);
}
