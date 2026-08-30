//! CC map tests.
//!
//! The worked examples in this file are taken verbatim from the "Mixer control via MIDI
//! CC" section of the ES-9 v1.3 user manual. They are the only authoritative statement
//! of how stereo links rewrite the CC mapping, and the rules are subtle enough that this
//! is exactly where a plausible-looking implementation goes wrong.

use es9_protocol::ccmap::{CcMap, Control};
use es9_protocol::links::Links;
use es9_protocol::tables::{Family, Source};

/// Mixer sources with no two channels in the same stereo pair.
fn plain_sources() -> [[Source; 8]; 2] {
    [
        core::array::from_fn(|c| Source::Usb(c as u8 + 1)),
        core::array::from_fn(|c| Source::Usb(c as u8 + 9)),
    ]
}

fn level_cc(map: &CcMap, mix: u8, channel: u8) -> Option<u8> {
    map.cc_for(mix, channel, Control::Level)
}

#[test]
fn base_scheme_increments_by_channel_then_by_mix() {
    let map = CcMap::resolve(Links(0), false, &plain_sources());

    // "The mix faders start at CC 0 for mix 1, channel 1 and increment first by
    // channel, then by mixer. So for example: mix 1 channel 8 is CC 7; mix 2 channel 1
    // is CC 8; mix 8 channel 8 is CC 63."
    assert_eq!(level_cc(&map, 0, 0), Some(0), "mix 1 channel 1");
    assert_eq!(level_cc(&map, 0, 7), Some(7), "mix 1 channel 8");
    assert_eq!(level_cc(&map, 1, 0), Some(8), "mix 2 channel 1");
    assert_eq!(level_cc(&map, 7, 7), Some(63), "mix 8 channel 8");
}

#[test]
fn without_mixer_2_the_upper_half_is_unassigned() {
    let map = CcMap::resolve(Links(0), false, &plain_sources());
    for cc in 64..128u8 {
        assert!(map.get(cc).is_none(), "CC {cc} should be unassigned");
    }
    assert_eq!(map.assigned().count(), 64);
}

#[test]
fn mixer_2_occupies_the_upper_half() {
    let map = CcMap::resolve(Links(0), true, &plain_sources());
    assert_eq!(level_cc(&map, 8, 0), Some(64), "mix 9 channel 1");
    assert_eq!(level_cc(&map, 15, 7), Some(127), "mix 16 channel 8");
    assert_eq!(map.assigned().count(), 128);
}

#[test]
fn stereo_mix_uses_the_lower_cc_for_level_and_the_upper_for_pan() {
    // "if mix 1/2 is stereo, then for mixer channel 1 use CC 0 (the CC for mix 1) not
    // CC 8 (the CC that would apply for channel 1 of mix 2)."
    // "If the mixer is stereo, the pan control uses the CC that would have been used by
    // the right channel mixer. E.g ... the pan control for channel 1 is CC 8."
    let mut links = Links(0);
    links.set(Family::Mix, 0, true);
    let map = CcMap::resolve(links, false, &plain_sources());

    let level = map.get(0).expect("CC 0 assigned");
    assert_eq!(level.control, Control::Level);
    assert!(level.mix.stereo, "mix 1/2 is a stereo pair");
    assert_eq!(level.mix.first, 0);
    assert_eq!(level.channel.first, 0);

    let pan = map.get(8).expect("CC 8 assigned");
    assert_eq!(pan.control, Control::Pan, "CC 8 becomes pan, not a level");
    assert_eq!(pan.mix.first, 0);
    assert_eq!(pan.channel.first, 0);

    // The level control is reachable from either half of the stereo pair.
    assert_eq!(level_cc(&map, 0, 0), Some(0));
    assert_eq!(level_cc(&map, 1, 0), Some(0), "not CC 8");
}

#[test]
fn stereo_input_collapses_two_channels_onto_the_lower_cc() {
    // "if USB 1/2 is stereo and these channels are inputs 1 & 2 of mixer 1, the use
    // CC 0 (the CC for input 1) not CC 1 (the CC for input 2)."
    let mut links = Links(0);
    links.set(Family::UsbLow, 0, true);
    let map = CcMap::resolve(links, false, &plain_sources());

    let a = map.get(0).expect("CC 0 assigned");
    assert!(a.channel.stereo, "channels 1 and 2 form one strip");
    assert_eq!(a.channel.first, 0);

    assert!(map.get(1).is_none(), "CC 1 is shadowed by the stereo link");
    assert_eq!(level_cc(&map, 0, 0), Some(0));
    assert_eq!(level_cc(&map, 0, 1), Some(0), "not CC 1");

    // Channels beyond the pair keep their own CCs.
    assert_eq!(level_cc(&map, 0, 2), Some(2));
}

#[test]
fn a_link_toggle_rewrites_the_map_and_the_diff_reports_it() {
    // This is the failure the hardware gives no warning about: enabling one stereo link
    // silently changes what existing controller mappings do.
    let before = CcMap::resolve(Links(0), false, &plain_sources());
    let mut links = Links(0);
    links.set(Family::Mix, 0, true);
    let after = CcMap::resolve(links, false, &plain_sources());

    let changes = before.diff(&after);
    assert!(!changes.is_empty(), "linking mix 1/2 must change the map");

    // Every CC in mix 2's row turns from a level into a pan.
    for cc in 8..16u8 {
        let change = changes
            .iter()
            .find(|c| c.cc == cc)
            .unwrap_or_else(|| panic!("CC {cc} should have changed"));
        assert_eq!(change.before.expect("was a level").control, Control::Level);
        assert_eq!(change.after.expect("now a pan").control, Control::Pan);
    }

    // Mix 1's row keeps its levels, but they now ride the stereo pair rather than
    // mix 1 alone -- so those CCs changed meaning too. Linking one pair of mixes
    // rewrites all sixteen CCs across both rows, which is precisely why this needs
    // surfacing rather than leaving the user to discover it mid-set.
    for cc in 0..8u8 {
        let change = changes
            .iter()
            .find(|c| c.cc == cc)
            .unwrap_or_else(|| panic!("CC {cc} should have changed"));
        let before = change.before.expect("was assigned");
        let after = change.after.expect("still assigned");
        assert_eq!(before.control, Control::Level);
        assert_eq!(after.control, Control::Level, "still a level");
        assert!(!before.mix.stereo, "was a mono mix");
        assert!(after.mix.stereo, "now a stereo pair");
    }

    assert_eq!(changes.len(), 16, "one link toggle rewrites both rows");
}

#[test]
fn stereo_mix_and_stereo_input_compose() {
    let mut links = Links(0);
    links.set(Family::Mix, 0, true);
    links.set(Family::UsbLow, 0, true);
    let map = CcMap::resolve(links, false, &plain_sources());

    let level = map.get(0).expect("CC 0 assigned");
    assert!(level.mix.stereo && level.channel.stereo);
    assert_eq!(level.control, Control::Level);

    assert!(map.get(1).is_none(), "shadowed by the stereo input");
    assert_eq!(
        map.get(8).expect("CC 8 assigned").control,
        Control::Pan,
        "pan for the collapsed strip"
    );
    assert!(map.get(9).is_none(), "pan for channel 2 is shadowed too");
}

#[test]
fn every_assigned_cc_is_reachable_from_its_control() {
    let mut links = Links(0);
    links.set(Family::Mix, 2, true);
    links.set(Family::UsbHigh, 4, true);
    let map = CcMap::resolve(links, true, &plain_sources());

    for (cc, a) in map.assigned() {
        let found = map.cc_for(a.mix.first, a.channel.first, a.control);
        assert_eq!(found, Some(cc), "CC {cc} should round-trip through cc_for");
    }
}
