//! Hardware probe: answers the checklist in `features/mixer-ui/plan.md` §8.
//!
//! Read-only by default. It archives a full configuration dump to disk before doing
//! anything else, because the plan requires an archive to exist before any write is
//! attempted. The write-side spikes (S1, H5) only run when asked for explicitly.

use std::io::Write as _;
use std::time::{Duration, Instant};

use es9_midi::{MidiLink, find_es9, list_ports};
use es9_protocol::config::DUMP_WORDS;
use es9_protocol::dcblock;
use es9_protocol::tables::{Destination, Family, Source};
use es9_protocol::{Incoming, decode, encode};

/// How long to wait for a reply before giving up on it.
const REPLY_TIMEOUT: Duration = Duration::from_millis(1500);

/// Gap between chunks of a `09H` configuration upload.
///
/// The reference tool sends all 24 with no pacing at all. This starts at the same place
/// so the test measures the protocol rather than our own caution.
const UPLOAD_GAP: Duration = Duration::from_millis(0);

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let write_tests = args.iter().any(|a| a == "--write-tests");
    let forced_in = flag_value(&args, "--in");
    let forced_out = flag_value(&args, "--out");
    // Just ask the module what rate it is running at and exit. Used to observe whether
    // opening a host audio stream re-clocks the module.
    let rate_only = args.iter().any(|a| a == "--rate-only");
    // `--set-macro <cc> <value>`: write one macro value and exit. Exists so a value this
    // probe moved can be put back without re-running the whole write suite, which would
    // capture the already-changed value as the "original".
    let set_macro = flag_value(&args, "--set-macro").zip(flag_value(&args, "--to"));
    // `--pan-sweep`: write a range of pan values and read each one back, to establish the
    // exact mapping between what is written and what the module stores.
    let pan_sweep = args.iter().any(|a| a == "--pan-sweep");

    let (ins, outs) = match list_ports() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("could not enumerate MIDI ports: {e}");
            std::process::exit(1);
        }
    };

    println!("== MIDI ports ==");
    println!("inputs:");
    for p in &ins {
        println!("  [{}] {}", p.index, p.name);
    }
    println!("outputs:");
    for p in &outs {
        println!("  [{}] {}", p.index, p.name);
    }

    // An explicit index wins: on Windows the ES-9 is published through Windows MIDI
    // Services under a generic jack name, so matching on "ES-9" does not find it.
    let in_index = forced_in.or_else(|| find_es9(&ins).map(|p| p.index));
    let out_index = forced_out.or_else(|| find_es9(&outs).map(|p| p.index));
    let (Some(in_index), Some(out_index)) = (in_index, out_index) else {
        eprintln!("\nno ES-9 port found by name. Re-run with --in <n> --out <n>.");
        std::process::exit(1);
    };
    let name_of = |ports: &[es9_midi::PortInfo], i: usize| {
        ports
            .iter()
            .find(|p| p.index == i)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "?".into())
    };
    println!(
        "\nusing input [{in_index}] {}, output [{out_index}] {}",
        name_of(&ins, in_index),
        name_of(&outs, out_index)
    );

    let mut link = match MidiLink::open(in_index, out_index) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("could not open: {e}");
            std::process::exit(1);
        }
    };

    // Version first: it is the cheapest proof the link works in both directions.
    if let Some((cc, value)) = set_macro {
        let (cc, value) = (cc as u8, value as u8);
        println!("setting macro CC {cc} = {value}");
        if let Err(e) = link.send(&encode::set_macro(cc, value)) {
            eprintln!("  send failed: {e}");
            std::process::exit(1);
        }
        std::thread::sleep(Duration::from_millis(300));
        match ask(&mut link, &encode::request_config()) {
            Some((Incoming::Config(c), _)) => {
                // The macro region is [0..128] levels by CC, [128..256] pans by CC.
                println!(
                    "  read back macro CC {cc} = {}",
                    c.macro_words[usize::from(cc)]
                );
            }
            _ => println!("  could not read back"),
        }
        return;
    }

    if pan_sweep {
        const LEVEL_CC: u8 = 0;
        const WRITE_CC: u8 = 8;
        let original = match ask(&mut link, &encode::request_mix()) {
            Some((Incoming::Mix(m), _)) => m.macro_cells[usize::from(LEVEL_CC)].aux,
            _ => {
                eprintln!("no mix dump");
                std::process::exit(1);
            }
        };
        println!("resting aux = {original}");
        println!(" written -> stored aux");
        for written in [0u8, 1, 32, 63, 64, 65, 96, 126, 127] {
            if let Err(e) = link.send(&encode::set_macro(WRITE_CC, written)) {
                eprintln!("  send failed: {e}");
            }
            std::thread::sleep(Duration::from_millis(250));
            match ask(&mut link, &encode::request_mix()) {
                Some((Incoming::Mix(m), _)) => {
                    let cell = m.macro_cells[usize::from(LEVEL_CC)];
                    let delta = i16::from(cell.aux) - i16::from(written);
                    println!("  {written:>3}   ->  {:>3}   (delta {delta})", cell.aux);
                }
                _ => println!("  {written:>3}   ->  no dump"),
            }
        }
        // Restore by writing whatever produces the original stored value.
        for candidate in 0..=127u8 {
            let _ = link.send(&encode::set_macro(WRITE_CC, candidate));
            std::thread::sleep(Duration::from_millis(120));
            if let Some((Incoming::Mix(m), _)) = ask(&mut link, &encode::request_mix())
                && m.macro_cells[usize::from(LEVEL_CC)].aux == original
            {
                println!("restored aux {original} by writing {candidate}");
                break;
            }
        }
        return;
    }

    if rate_only {
        match ask(&mut link, &encode::request_sample_rate()) {
            Some((Incoming::SampleRate(hz), _)) => println!("SAMPLE RATE: {hz} Hz"),
            Some((other, _)) => println!("SAMPLE RATE: unexpected {other:?}"),
            None => println!("SAMPLE RATE: no reply"),
        }
        return;
    }

    println!("\n== version ==");
    // The first request after opening a port is sometimes lost while the endpoint is
    // still settling, so this one gets a second chance before being called a failure.
    let mut version = ask(&mut link, &encode::request_version());
    if version.is_none() {
        std::thread::sleep(Duration::from_millis(300));
        version = ask(&mut link, &encode::request_version());
    }
    match version {
        Some((Incoming::Message(m), _)) => println!("  {m}"),
        Some((other, cmd)) => println!("  unexpected reply {other:?} (cmd {cmd:#04X})"),
        None => println!("  NO REPLY after two attempts"),
    }

    println!("\n== sample rate ==");
    match ask(&mut link, &encode::request_sample_rate()) {
        Some((Incoming::SampleRate(hz), _)) => println!("  {hz} Hz"),
        Some((other, _)) => println!("  unexpected: {other:?}"),
        None => println!("  no reply"),
    }

    // ---- H1: dump command byte and word count ----
    println!("\n== H1: configuration dump ==");
    let raw = ask_raw(&mut link, &encode::request_config());
    let Some(bytes) = raw else {
        eprintln!("  NO DUMP RECEIVED — cannot continue");
        std::process::exit(1);
    };
    let cmd = bytes.get(5).copied().unwrap_or(0);
    // Header is 6 bytes, trailer is 1; the rest is 3 bytes per 21-bit word.
    let payload = bytes.len().saturating_sub(7);
    println!("  {} bytes, command {cmd:#04X}", bytes.len());
    println!(
        "  payload {payload} bytes = {} words (expected {DUMP_WORDS})",
        payload / 3
    );
    println!(
        "  H1 {}",
        if cmd == 0x08 && payload / 3 == DUMP_WORDS {
            "PASS"
        } else {
            "FAIL"
        }
    );

    archive(&bytes);

    let config = match decode(&bytes) {
        Ok(Incoming::Config(c)) => c,
        Ok(other) => {
            eprintln!("  dump decoded as {other:?}, not a config");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("  DECODE FAILED: {e}");
            std::process::exit(1);
        }
    };
    println!("  decoded. version field = {}", config.version);

    // ---- H6 / H7: routing permutations ----
    println!("\n== H6/H7: routing as the module reports it ==");
    for block in 0..4 {
        let row: Vec<String> = (0..8)
            .map(|ch| match config.capture_source(block, ch) {
                Some(s) => label_source(s),
                None => format!("?{}", config.capture[block][ch]),
            })
            .collect();
        println!("  capture block {block}: {}", row.join(", "));
    }
    for block in 0..4 {
        let row: Vec<String> = (0..8)
            .map(|ch| match config.output_destination(block, ch) {
                Some(d) => format!("{d:?}"),
                None => format!("?{}", config.outputs[block][ch]),
            })
            .collect();
        println!("  output  block {block}: {}", row.join(", "));
    }
    println!("  H6/H7: compare the above against the physical patch by ear.");

    // ---- options, links, MIDI channels ----
    println!("\n== options ==");
    println!("  raw bits {:#010X}", config.options.to_bits());
    println!(
        "  MIDI channel: USB {}, DIN {}",
        config.midi_usb, config.midi_din
    );
    println!("  smoothing bitmap {:#06X}", config.smoothing);
    let links = config.links;
    let linked: Vec<String> = [
        Family::Input,
        Family::UsbLow,
        Family::UsbHigh,
        Family::Mix,
        Family::SpdifOrMix2,
        Family::Bus,
    ]
    .iter()
    .flat_map(|&f| {
        (0..8)
            .step_by(2)
            .filter(move |i| links.is_linked(f, *i))
            .map(move |i| format!("{f:?}{}/{}", i + 1, i + 2))
    })
    .collect();
    println!(
        "  stereo links: {}",
        if linked.is_empty() {
            "none".into()
        } else {
            linked.join(", ")
        }
    );

    // ---- H2 / Q1: macro word region ----
    //
    // The hypothesis this tests: the 256 words are two parallel arrays of 128, indexed
    // by CC — levels in the low half, the pan/aux value in the high half. If it holds,
    // the region stops being opaque and the dump alone can drive the mixer UI.
    println!("\n== H2/Q1: macro-mix region ==");
    let show = |label: &str, range: std::ops::Range<usize>| {
        let row: Vec<String> = config.macro_words[range]
            .iter()
            .map(|w| format!("{w}"))
            .collect();
        println!("  {label}: {}", row.join(" "));
    };
    show("words   0..16 ", 0..16);
    show("words 120..136", 120..136);
    show("words 240..256", 240..256);
    let nonzero = config.macro_words.iter().filter(|w| **w != 0).count();
    println!("  {nonzero}/256 words non-zero");
    let low_nonzero = config.macro_words[..128]
        .iter()
        .filter(|w| **w != 0)
        .count();
    let high_64 = config.macro_words[128..]
        .iter()
        .filter(|w| **w == 64)
        .count();
    println!("  low half: {low_nonzero}/128 non-zero; high half: {high_64}/128 equal to 64");
    if high_64 == 128 {
        println!("  Q1: consistent with [0..128] = level by CC, [128..256] = pan by CC.");
    }

    // ---- H9: reserved tail ----
    let res_nonzero = config.reserved.iter().filter(|w| **w != 0).count();
    println!(
        "\n== H9: reserved tail == {res_nonzero}/{} words non-zero",
        config.reserved.len()
    );

    // ---- H3 / H4: mix dump, pan rows and centre bias ----
    println!("\n== H3/H4: mix dump ==");
    match ask(&mut link, &encode::request_mix()) {
        Some((Incoming::Mix(mix), cmd)) => {
            println!("  command {cmd:#04X}");
            println!("  macro cells 0..16 (CC = mix*8 + channel):");
            for cc in 0..16u8 {
                let c = &mix.macro_cells[usize::from(cc)];
                println!("    CC {cc:>3}: {c:?}");
            }
            println!("  raw mix rows 0..2:");
            for m in 0..2usize {
                println!("    mix {m}: {:?}", mix.raw[m]);
            }
            println!("  H4: read the pan cell's resting value above — 63 or 64 decides the bias.");
        }
        Some((other, _)) => println!("  unexpected: {other:?}"),
        None => println!("  no reply"),
    }

    println!("\n== DSP usage ==");
    match ask(&mut link, &encode::request_usage()) {
        Some((Incoming::Usage(u), _)) => {
            println!(
                "  dsp0 {:.1}%",
                es9_protocol::decode::Usage::percent(u.dsp0)
            );
            if let Some(d1) = u.dsp1 {
                println!("  dsp1 {:.1}%", es9_protocol::decode::Usage::percent(d1));
            }
        }
        Some((other, _)) => println!("  unexpected: {other:?}"),
        None => println!("  no reply"),
    }

    if !write_tests {
        println!("\n== write-side spikes skipped ==");
        println!("  A dump has been archived. Re-run with --write-tests to run S1 and H5,");
        println!("  which change live (hosted) mixer state but never touch flash.");
        return;
    }

    // ---- H10: DC-blocking filter pairs ----
    //
    // The bitmap is documented as one bit per input *pair*, with a non-sequential
    // pairing taken from the official tool: bit 1 covers inputs 3 and 8. This walks one
    // bit at a time and reads the whole configuration back, which confirms the field is
    // seven bits wide and that a write survives, but cannot by itself prove which
    // physical inputs a bit governs — that needs a CV on an input and a look at the
    // captured signal. What it does prove is that we can address the filters
    // individually without disturbing the rest.
    println!("\n== H10: DC-blocking filters ==");
    let original_hpf = config.hpf as u8;
    println!("  original bitmap {original_hpf:#04X} ({original_hpf:07b})");
    for bit in 0..dcblock::PAIR_COUNT as u8 {
        let label = dcblock::label(bit).unwrap_or_default();
        let state = if dcblock::is_on(original_hpf, bit) {
            "on"
        } else {
            "off"
        };
        println!("    bit {bit}: {label:<14} {state}");
    }

    // Walk a single bit through the field and read each step back.
    let mut walked_ok = true;
    for bit in 0..dcblock::PAIR_COUNT as u8 {
        let want = dcblock::with(0, bit, true);
        if let Err(e) = link.send(&encode::set_hpf(want)) {
            eprintln!("  send failed: {e}");
        }
        std::thread::sleep(Duration::from_millis(150));
        match read_hpf(&mut link) {
            Some(got) if got == want => {}
            Some(got) => {
                println!("    bit {bit}: wrote {want:#04X}, read {got:#04X}  MISMATCH");
                walked_ok = false;
            }
            None => {
                println!("    bit {bit}: no dump came back");
                walked_ok = false;
            }
        }
    }

    // An eighth bit should not stick: the field is documented as seven bits.
    if let Err(e) = link.send(&encode::set_hpf(0xFF)) {
        eprintln!("  send failed: {e}");
    }
    std::thread::sleep(Duration::from_millis(150));
    let all_bits = read_hpf(&mut link);
    println!("  wrote 0xFF, read back {all_bits:?}");
    match all_bits {
        Some(0x7F) => println!("  the field is seven bits, as documented"),
        Some(other) => println!("  field width is NOT seven bits: got {other:#04X}"),
        None => println!("  no dump came back"),
    }
    println!(
        "  H10 round trip: {}",
        if walked_ok { "PASS" } else { "FAIL" }
    );

    // Put the user's filters back.
    let _ = link.send(&encode::set_hpf(original_hpf));
    std::thread::sleep(Duration::from_millis(150));
    match read_hpf(&mut link) {
        Some(got) if got == original_hpf => println!("  restored {original_hpf:#04X}"),
        Some(got) => println!("  RESTORE FAILED: wanted {original_hpf:#04X}, got {got:#04X}"),
        None => println!("  RESTORE UNVERIFIED: no dump came back"),
    }

    // ---- H5 / Q6: does the module echo our own writes? ----
    println!("\n== H5/Q6: echo ==");
    let before = link.poll().len();
    let _ = before;
    link.monitor_mut().clear();
    if let Err(e) = link.send(&encode::set_macro(0, 100)) {
        eprintln!("  send failed: {e}");
    }
    let replies = collect(&mut link, Duration::from_millis(600));
    if replies.is_empty() {
        println!("  no echo. Q6 = the module does not echo host writes.");
    } else {
        println!("  {} message(s) came back:", replies.len());
        for r in &replies {
            println!(
                "    cmd {:#04X}: {:?}",
                r.get(5).copied().unwrap_or(0),
                decode(r)
            );
        }
    }

    // ---- H8 / Q4: is the chunked config upload reliable? ----
    //
    // `09H` sends 24 chunks of 32 words with no acknowledgement and no flow control. If
    // it round-trips, a preset can be applied as one complete configuration; if it drops
    // chunks, presets have to be rebuilt from individual commands instead. Uploading the
    // configuration the module already has makes this safe: a success changes nothing,
    // and a failure is visible as a difference from a dump we have archived.
    println!("\n== H8/Q4: chunked config upload ==");
    let chunks = encode::config_upload(&config);
    println!("  uploading {} chunks of {} words", chunks.len(), 32);
    for chunk in &chunks {
        if let Err(e) = link.send(chunk) {
            eprintln!("  send failed: {e}");
        }
        // No pacing at all is what the reference tool does; a small gap is the first
        // thing to try if this proves unreliable, so it is a named knob rather than a
        // magic sleep.
        std::thread::sleep(UPLOAD_GAP);
    }
    std::thread::sleep(Duration::from_millis(500));
    match ask(&mut link, &encode::request_config()) {
        Some((Incoming::Config(after), _)) => {
            if *after == *config {
                println!("  H8 PASS — the configuration came back identical.");
            } else {
                println!("  H8 FAIL — the configuration changed. Differences:");
                if after.hpf != config.hpf {
                    println!("    hpf {:#04X} -> {:#04X}", config.hpf, after.hpf);
                }
                for b in 0..4 {
                    if after.capture[b] != config.capture[b] {
                        println!(
                            "    capture {b}: {:?} -> {:?}",
                            config.capture[b], after.capture[b]
                        );
                    }
                    if after.outputs[b] != config.outputs[b] {
                        println!(
                            "    outputs {b}: {:?} -> {:?}",
                            config.outputs[b], after.outputs[b]
                        );
                    }
                }
                if after.options != config.options {
                    println!("    options changed");
                }
                if after.macro_words != config.macro_words {
                    let differing = (0..256)
                        .filter(|i| after.macro_words[*i] != config.macro_words[*i])
                        .count();
                    println!("    macro words: {differing}/256 differ");
                }
                if after.raw_mix != config.raw_mix {
                    println!("    raw mix differs");
                }
            }
        }
        _ => println!("  no configuration dump came back after the upload"),
    }

    // ---- H3: where does a stereo pair's pan actually live? ----
    //
    // The manual says pan for a stereo pair (m, m+1) is written to CC (m+1)*8 + ch. What
    // it does not say is where it is *read* from, and the reference tool reads a
    // different place than it writes — so one of its two halves is wrong and the doc
    // cannot settle which.
    //
    // Every `aux` byte in a resting mix dump reads 64, which is exactly pan centre, so
    // the suspicion is that pan lives in the level cell's `aux` and CC (m+1)*8+ch is a
    // write-only address. This writes a distinctive pan and looks in both places.
    println!("\n== H3: pan storage ==");
    const PAN_LEVEL_CC: u8 = 0; // mix 1/2, channel 1 level
    const PAN_WRITE_CC: u8 = 8; // where the manual says to write its pan
    const PAN_PROBE: u8 = 100; // distinctive, and clearly right-of-centre

    let before = ask(&mut link, &encode::request_mix());
    let (before_level_aux, before_write_value) = match &before {
        Some((Incoming::Mix(m), _)) => (
            m.macro_cells[usize::from(PAN_LEVEL_CC)].aux,
            m.macro_cells[usize::from(PAN_WRITE_CC)].value,
        ),
        _ => (64, 0),
    };
    println!(
        "  before: CC {PAN_LEVEL_CC} aux = {before_level_aux}, CC {PAN_WRITE_CC} value = {before_write_value}"
    );

    if let Err(e) = link.send(&encode::set_macro(PAN_WRITE_CC, PAN_PROBE)) {
        eprintln!("  send failed: {e}");
    }
    std::thread::sleep(Duration::from_millis(400));
    match ask(&mut link, &encode::request_mix()) {
        Some((Incoming::Mix(m), _)) => {
            let level_cell = m.macro_cells[usize::from(PAN_LEVEL_CC)];
            let write_cell = m.macro_cells[usize::from(PAN_WRITE_CC)];
            println!("  wrote {PAN_PROBE} to CC {PAN_WRITE_CC}");
            println!("  after:  CC {PAN_LEVEL_CC} = {level_cell:?}");
            println!("  after:  CC {PAN_WRITE_CC} = {write_cell:?}");
            if level_cell.aux == PAN_PROBE {
                println!("  H3: pan is READ from the level cell's aux byte.");
            } else if write_cell.value == PAN_PROBE {
                println!("  H3: pan is READ from the value of CC {PAN_WRITE_CC}, as written.");
            } else {
                println!("  H3: the write landed in NEITHER place — pan storage is elsewhere.");
            }
        }
        _ => println!("  no mix dump came back"),
    }

    // Put the pan back to where it was.
    let _ = link.send(&encode::set_macro(PAN_WRITE_CC, before_level_aux));
    std::thread::sleep(Duration::from_millis(200));
    println!("  restored pan to {before_level_aux}");

    // ---- S1: does a raw mix write survive the macro layer? ----
    println!("\n== S1: raw mix persistence ==");
    const PROBE_MIX: u8 = 0;
    const PROBE_CH: u8 = 0;
    const PROBE_VALUE: u16 = 0x4000;
    let original = config.raw_mix[usize::from(PROBE_MIX)][usize::from(PROBE_CH)];
    let probe_cc = PROBE_MIX * 8 + PROBE_CH;
    // Readable because the macro region is [0..128] levels by CC (H2).
    let original_macro = config.macro_words[usize::from(probe_cc)] as u8;
    println!("  original raw[{PROBE_MIX}][{PROBE_CH}] = {original:#06X}");
    println!("  original macro CC {probe_cc} = {original_macro}");

    if let Err(e) = link.send(&encode::set_raw_mix(PROBE_MIX, PROBE_CH, PROBE_VALUE)) {
        eprintln!("  send failed: {e}");
    }
    std::thread::sleep(Duration::from_millis(400));
    let after_write = read_raw_cell(&mut link, PROBE_MIX, PROBE_CH);
    println!("  wrote {PROBE_VALUE:#06X}, read back {after_write:?}");

    // Now move the macro fader on the same cell and see whether raw follows it.
    if let Err(e) = link.send(&encode::set_macro(probe_cc, 100)) {
        eprintln!("  send failed: {e}");
    }
    std::thread::sleep(Duration::from_millis(400));
    let after_macro = read_raw_cell(&mut link, PROBE_MIX, PROBE_CH);
    println!("  after a macro write, raw = {after_macro:?}");
    match (after_write, after_macro) {
        (Some(a), Some(b)) if a == PROBE_VALUE && b == a => {
            println!("  S1: raw HOLDS. The advanced raw panel is meaningful.");
        }
        (Some(a), Some(b)) if a == PROBE_VALUE && b != a => {
            println!("  S1: raw held, then the macro layer overwrote it. Macro drives raw.");
        }
        (Some(a), _) if a != PROBE_VALUE => {
            println!("  S1: the raw write did not stick at all. Macro drives raw. Drop step 15.");
        }
        _ => println!("  S1: inconclusive — no mix dump came back."),
    }

    // Put it back the way we found it. The macro goes back first: since macro drives
    // raw, restoring the raw cell before the macro would immediately be undone by it.
    let _ = link.send(&encode::set_macro(probe_cc, original_macro));
    std::thread::sleep(Duration::from_millis(200));
    let _ = link.send(&encode::set_raw_mix(PROBE_MIX, PROBE_CH, original));
    std::thread::sleep(Duration::from_millis(200));
    match read_raw_cell(&mut link, PROBE_MIX, PROBE_CH) {
        Some(got) => println!(
            "\n  restored macro CC {probe_cc} = {original_macro}, raw now {got:#06X} (was {original:#06X})"
        ),
        None => println!("\n  restore unverified: no mix dump came back"),
    }
}

/// Reads a `--flag <n>` numeric argument.
fn flag_value(args: &[String], flag: &str) -> Option<usize> {
    let at = args.iter().position(|a| a == flag)?;
    args.get(at + 1)?.parse().ok()
}

/// Sends a request and returns the first decodable reply, with its command byte.
fn ask(link: &mut MidiLink, request: &[u8]) -> Option<(Incoming, u8)> {
    let bytes = ask_raw(link, request)?;
    let cmd = bytes.get(5).copied().unwrap_or(0);
    decode(&bytes).ok().map(|i| (i, cmd))
}

/// Sends a request and returns the first raw message that comes back.
fn ask_raw(link: &mut MidiLink, request: &[u8]) -> Option<Vec<u8>> {
    // Drain anything already queued so a stale reply is not mistaken for this one.
    let _ = link.poll();
    if let Err(e) = link.send(request) {
        eprintln!("  send failed: {e}");
        return None;
    }
    let deadline = Instant::now() + REPLY_TIMEOUT;
    while Instant::now() < deadline {
        let messages = link.poll();
        if let Some(m) = messages.into_iter().next() {
            return Some(m);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    None
}

/// Collects everything that arrives within a window.
fn collect(link: &mut MidiLink, window: Duration) -> Vec<Vec<u8>> {
    let deadline = Instant::now() + window;
    let mut out = Vec::new();
    while Instant::now() < deadline {
        out.extend(link.poll());
        std::thread::sleep(Duration::from_millis(10));
    }
    out
}

/// Re-reads the DC-blocking bitmap via a fresh configuration dump.
fn read_hpf(link: &mut MidiLink) -> Option<u8> {
    match ask(link, &encode::request_config()) {
        Some((Incoming::Config(c), _)) => Some(c.hpf as u8),
        _ => None,
    }
}

/// Re-reads one raw mixer cell via a fresh mix dump.
fn read_raw_cell(link: &mut MidiLink, mix: u8, channel: u8) -> Option<u16> {
    match ask(link, &encode::request_mix()) {
        Some((Incoming::Mix(m), _)) => Some(m.raw[usize::from(mix)][usize::from(channel)]),
        _ => None,
    }
}

/// Writes the dump to a timestamped file so there is always a way back.
fn archive(bytes: &[u8]) {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let name = format!("es9-dump-{stamp}.syx");
    match std::fs::File::create(&name).and_then(|mut f| f.write_all(bytes)) {
        Ok(()) => println!("  archived {} bytes to {name}", bytes.len()),
        Err(e) => eprintln!("  COULD NOT ARCHIVE the dump: {e}"),
    }
}

fn label_source(s: Source) -> String {
    format!("{s:?}")
}

// Keeps the unused-import warning honest when Destination is only used in a match arm.
const _: Option<Destination> = None;
