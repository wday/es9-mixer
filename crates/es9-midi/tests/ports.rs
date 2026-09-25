//! Port-name matching, which decides which pairs autoconnect tries first.

use es9_midi::looks_like_es9;

#[test]
fn coremidi_names_match() {
    assert!(looks_like_es9("ES-9"));
    assert!(looks_like_es9("Expert Sleepers ES-9 MIDI"));
    assert!(looks_like_es9("es9"));
}

#[test]
fn windows_jack_names_do_not() {
    // Windows MIDI Services publishes the module under a generic jack name, which is why
    // autoconnect still falls back to asking every pair.
    assert!(!looks_like_es9("2 - MIDI In"));
    assert!(!looks_like_es9("IAC Driver Bus 1"));
}
