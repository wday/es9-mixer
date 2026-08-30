//! ES-9 Mixer's own default configurations — **not** the module's factory defaults.
//!
//! Three things in this project are easy to confuse, so they are named apart:
//!
//! | | What it is |
//! |---|---|
//! | **Factory defaults** | The module's own, applied by `26H`. Nothing here produces them. |
//! | **Mixer defaults** (this module) | This project's opinionated pair, below. |
//! | **User presets** (`es9-app::presets`) | Saved `08H` dumps in `.syx` files on disk. |
//!
//! These two are modelled on the factory pair and differ from them deliberately — see
//! `base()` for the capture 15/16 deviation. They are the starting point for the offline
//! mock and the browser prototype; **the desktop app never applies them to a module**,
//! because "reset to defaults" sends `26H` and lets the module apply its own.
//!
//! The pair differ from each other in what feeds the mixer, which is the point: with no
//! computer attached USB playback is silent, so the standalone patch takes the analogue
//! inputs instead.

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
    //
    // **This deviates from the module's own default, deliberately.** The factory config
    // puts the S/PDIF input on capture 15/16 (manual p.11). Pointing them at MIX 1/2
    // instead gives the DAW a stereo capture of exactly what leaves the main outs,
    // sample-aligned with the 14 multitracks — a reference mix of what was actually
    // heard, which is worth more on a recording rig than an S/PDIF return.
    //
    // The cost is that the S/PDIF *input* no longer reaches the host. Anyone who needs
    // it puts `Source::Spdif` back here; do not "correct" this to match the factory
    // default without knowing which trade is wanted.
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

/// **Mixer hosted default**: the mixer takes USB playback, block 3 is the S/PDIF
/// processor carrying DAW 5-6 out.
pub fn hosted() -> Config {
    let mut c = base();
    c.options = Options {
        mixer2: false,
        midi_thru: false,
    };
    let sources: Vec<Source> = (1..=8).map(Source::Usb).collect();
    c.capture[2] = wire(&sources, Source::to_wire);
    // Block 3's capture side is what feeds the S/PDIF *output*, so it takes DAW channels
    // 5-6: "the S/PDIF block has its inputs set to USB 5-6 i.e. DAW outputs 5-6 get sent
    // out to the S/PDIF output" (manual p.11). Feeding it from `Source::Spdif` instead
    // would make the optical output a passthrough of the optical input, which is not
    // what anyone wants and is not the module's default.
    c.capture[3] = wire(&[Source::Usb(5), Source::Usb(6)], Source::to_wire);
    c
}

/// **Mixer standalone default**: the mixer takes the analogue inputs, and block 3 is the
/// second mixer bank rather than the S/PDIF processor.
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
