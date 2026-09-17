//! Meter ballistics.

use es9_meter::{Meter, linear_to_db};

/// One block of a constant amplitude, at a rate and block size close to what the ES-9
/// actually delivers over ASIO.
fn meter() -> Meter {
    Meter::new(48_000, 512)
}

#[test]
fn silence_is_negative_infinity() {
    let m = meter();
    assert_eq!(m.peak_db(), f32::NEG_INFINITY);
    assert_eq!(m.rms_db(), f32::NEG_INFINITY);
}

#[test]
fn full_scale_is_zero_dbfs() {
    assert_eq!(linear_to_db(1.0), 0.0);
    assert!((linear_to_db(0.5) + 6.02).abs() < 0.01);
}

#[test]
fn peak_attacks_immediately() {
    let mut m = meter();
    m.push([0.0, 0.5, -0.25]);
    // Attack is instantaneous, so one block is enough to reach the block's peak.
    assert!((m.peak - 0.5).abs() < 1e-6, "peak was {}", m.peak);
}

#[test]
fn peak_decays_but_hold_does_not() {
    let mut m = meter();
    m.push([0.8]);
    let after_attack = m.peak;
    for _ in 0..20 {
        m.push([0.0]);
    }
    assert!(m.peak < after_attack, "peak should fall towards silence");
    assert!((m.hold - 0.8).abs() < 1e-6, "hold should stay at the peak");
}

#[test]
fn reset_hold_drops_to_current_peak() {
    let mut m = meter();
    m.push([0.9]);
    for _ in 0..50 {
        m.push([0.1]);
    }
    m.reset_hold();
    assert!((m.hold - m.peak).abs() < 1e-6);
    assert!(!m.clipped);
}

#[test]
fn clipping_is_sticky_until_reset() {
    let mut m = meter();
    m.push([1.0]);
    assert!(m.clipped, "full scale must register as a clip");
    m.push([0.01]);
    assert!(m.clipped, "the clip flag must survive quieter blocks");
    m.reset_hold();
    assert!(!m.clipped);
}

#[test]
fn rms_of_full_scale_square_approaches_unity() {
    let mut m = meter();
    // A square wave at full scale has an RMS equal to its amplitude.
    for _ in 0..200 {
        m.push([1.0, -1.0, 1.0, -1.0]);
    }
    assert!((m.rms - 1.0).abs() < 0.01, "rms was {}", m.rms);
}

#[test]
fn rms_is_below_peak_for_a_sine() {
    let mut m = meter();
    let block: Vec<f32> = (0..512)
        .map(|i| (i as f32 * std::f32::consts::TAU / 64.0).sin())
        .collect();
    for _ in 0..200 {
        m.push(block.iter().copied());
    }
    // A sine's RMS is 1/sqrt(2) of its peak: about -3 dB.
    let difference = m.peak_db() - m.rms_db();
    assert!(
        (difference - 3.01).abs() < 0.2,
        "peak {} dB, rms {} dB",
        m.peak_db(),
        m.rms_db()
    );
}

#[test]
fn an_empty_block_changes_nothing() {
    let mut m = meter();
    m.push([0.7]);
    let before = (m.peak, m.rms, m.hold);
    m.push(std::iter::empty());
    assert_eq!((m.peak, m.rms, m.hold), before);
}

#[test]
fn silence_has_no_dc() {
    let mut m = meter();
    for _ in 0..200 {
        m.push([0.0, 0.0, 0.0, 0.0]);
    }
    assert!(m.dc.abs() < 1e-6, "dc was {}", m.dc);
}

#[test]
fn a_constant_offset_shows_up_as_dc() {
    let mut m = meter();
    for _ in 0..400 {
        m.push([0.25, 0.25, 0.25, 0.25]);
    }
    assert!((m.dc - 0.25).abs() < 0.01, "dc was {}", m.dc);
}

#[test]
fn dc_is_signed_so_the_trim_direction_is_visible() {
    let mut m = meter();
    for _ in 0..400 {
        m.push([-0.1, -0.1, -0.1, -0.1]);
    }
    assert!(
        m.dc < 0.0,
        "a negative offset must read negative, got {}",
        m.dc
    );
}

#[test]
fn a_centred_signal_has_no_dc_even_at_full_scale() {
    // Peak and RMS both discard sign, so only the mean distinguishes a loud centred
    // signal from a quiet offset one. This is the whole reason dc exists.
    let mut m = meter();
    for _ in 0..400 {
        m.push([1.0, -1.0, 1.0, -1.0]);
    }
    assert!(m.dc.abs() < 0.01, "dc was {}", m.dc);
    assert!((m.rms - 1.0).abs() < 0.01, "rms should still be full scale");
}

#[test]
fn an_offset_signal_reports_both_level_and_dc() {
    let mut m = meter();
    // A square wave riding on a constant offset.
    for _ in 0..400 {
        m.push([0.6, 0.2, 0.6, 0.2]);
    }
    assert!((m.dc - 0.4).abs() < 0.01, "dc was {}", m.dc);
}
