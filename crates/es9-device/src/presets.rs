//! The two default configurations the module ships with.
//!
//! The manual describes these as the targets of its two "reset to defaults" buttons: one
//! suited to hosted use, one to standalone. They differ in what feeds the mixer, which is
//! the whole point — with no computer attached, USB playback is silent, so a standalone
//! patch takes the analogue inputs instead.

use es9_protocol::config::{Config, Options};
use es9_protocol::tables::{Destination, Source};

/// Where mixes 1-8 are sent in both defaults.
const MIX_OUTPUTS: [Destination; 8] = [
    Destination::MainOut(false),
    Destination::MainOut(true),
    Destination::Phones(false),
    Destination::Phones(true),
    Destination::Output(1),
    Destination::Output(2),
    Destination::Output(3),
    Destination::Output(4),
];

/// Bus 16 is the module's conventional dead end for an output that must go somewhere.
const DEAD_END: Destination = Destination::Bus(16);

fn wire<T: Copy>(items: &[T], f: impl Fn(T) -> Option<u8>) -> [u8; 8] {
    let mut out = [0u8; 8];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = items.get(i).and_then(|v| f(*v)).unwrap_or(0);
    }
    out
}

fn base() -> Config {
    // Every DC-blocking filter on. The manual does not state what "reset to defaults"
    // does with these, so this is taken from what a real module was found holding, and
    // it matches the manual's advice to leave them on for audio. Anyone patching CV has
    // to turn the relevant pair off — which is the whole reason the Inputs panel exists.
    let mut c = Config {
        hpf: 0x7F,
        ..Default::default()
    };

    // USB capture: inputs 1-14 to the computer, then the two mixer outputs.
    let mut usb_sources = Vec::new();
    for n in 1..=14u8 {
        usb_sources.push(Source::Input(n));
    }
    usb_sources.push(Source::Mix(1));
    usb_sources.push(Source::Mix(2));
    c.capture[0] = wire(&usb_sources[..8], Source::to_wire);
    c.capture[1] = wire(&usb_sources[8..], Source::to_wire);

    // USB playback: channels 9-16 drive the eight 3.5mm outputs, 7-8 the ES-5. Channels
    // 1-6 are handled by the mixer, so they are parked on the dead end.
    let block0: Vec<Destination> = (0..6)
        .map(|_| DEAD_END)
        .chain([Destination::Es5(false), Destination::Es5(true)])
        .collect();
    let block1: Vec<Destination> = (1..=8).map(Destination::Output).collect();
    c.outputs[0] = wire(&block0, Destination::to_wire);
    c.outputs[1] = wire(&block1, Destination::to_wire);

    // Mixer block 2 feeds the main outs, phones and the first four outputs.
    c.outputs[2] = wire(&MIX_OUTPUTS, Destination::to_wire);
    c.outputs[3] = wire(&[DEAD_END; 8], Destination::to_wire);

    // Mix 1/2 passes its first stereo pair through at unity.
    for mix in 0..2usize {
        c.raw_mix[mix][mix] = es9_protocol::scales::RAW_UNITY;
    }
    c
}

/// Defaults suited to hosted use: the mixer takes USB playback, S/PDIF is enabled.
pub fn hosted() -> Config {
    let mut c = base();
    c.options = Options {
        mixer2: false,
        midi_thru: false,
    };
    let sources: Vec<Source> = (1..=8).map(Source::Usb).collect();
    c.capture[2] = wire(&sources, Source::to_wire);
    c.capture[3] = wire(
        &[Source::Spdif(false), Source::Spdif(true)],
        Source::to_wire,
    );
    c
}

/// Defaults suited to standalone use: the mixer takes the analogue inputs, and the
/// second mixer is enabled.
///
/// This is the starting point for a DAW-free rig, where the module runs from flash and a
/// MIDI controller drives the macro mix.
pub fn standalone() -> Config {
    let mut c = base();
    c.options = Options {
        mixer2: true,
        midi_thru: true,
    };
    let first: Vec<Source> = (1..=8).map(Source::Input).collect();
    let second: Vec<Source> = (9..=14).map(Source::Input).collect();
    c.capture[2] = wire(&first, Source::to_wire);
    c.capture[3] = wire(&second, Source::to_wire);
    c.outputs[3] = wire(&MIX_OUTPUTS, Destination::to_wire);
    c
}
