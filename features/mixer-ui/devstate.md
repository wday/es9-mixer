# ES-9 Mixer — Development State

Where the project stands. Present tense only: what is built, what has been measured
against hardware, what must not be broken, and what is still unverified.

**This file has no history.** The development narrative — every hypothesis, the ones that
turned out wrong, and how each was settled — lives in git. `git log -- features/mixer-ui/`
if you need the archaeology; do not reconstruct it here.

## What exists

| Crate | Role | Licence |
|---|---|---|
| `es9-protocol` | SysEx codec for the firmware 1.3 word-array dump. Pure: no I/O. | MIT |
| `es9-device` | State, actions, undo, offline mock, send coalescing, presentation model. Pure. | MIT |
| `es9-midi` | `midir` transport, SysEx reassembly, protocol monitor, `probe` binary. | MIT |
| `es9-audio` | ASIO/WASAPI capture, meter ballistics, `audioprobe` binary. | **GPL-3.0** |
| `es9-app` | Tauri desktop shell. Its own workspace. | **GPL-3.0** |
| `es9-wasm` | Browser bridge driving the mock. | MIT |
| `web/` | One frontend; picks Tauri `invoke` or the WASM mock at load time. | — |

112 tests, clippy and rustfmt clean, `./scripts/smoke.sh` passing.

The shell runs on the rig: it connects over MIDI, shows the module's real configuration,
and meters sixteen channels at 48 kHz over ASIO. It is installed via
`scripts/win-install.ps1` and launched from the Start Menu.

Tabs: **Mixer**, **Routing**, **Analogue**, **Meters**, **CC Map** (carrying the stereo
links, because a link rewrites the map it sits above), **Presets**, **Monitor**.

### The routing patchbay

Routing is drawn by `web/patchbay.js` as two bays on one tab, in signal order: sources
into capture channels, then block outputs into destinations. Each bay is a row of jacks
above a row of jacks with SVG cables strung between them. Click a jack, click one on the
far row, and the connection is written; Escape abandons a half-made patch.

The shape is chosen to carry facts a grid of cells cannot:

- **A jack holds one plug.** Every capture channel has exactly one source and every block
  output exactly one destination, so repatching visibly *moves* a plug. In a grid that
  rule is invisible and has to be written underneath in prose.
- **Both rows share one pitch and start at the same offset**, so Input 3 sits directly
  above USB in 3 and a straight-through patch is a vertical cable. Deviation from the
  obvious routing is then a visible diagonal. This is why family gaps are drawn as
  labelled bands above and below the rows rather than as gaps in the rows: a gap would
  break that alignment, because the source families (14/16/16/8/2) and the channel blocks
  (8/8/8/8) do not divide at the same places.
- **A jack carrying more than one cable is ringed.** On the source row that is fan-out;
  on the destination row it is the module summing, which is how both mixer banks reach
  the main outs and is easy to create by accident.
- **A plug takes its cable's colour, not its jack's.** On the channel row those differ,
  and the arriving signal is the thing worth naming. Where families sum onto one
  destination no single colour is true, so that plug goes neutral.
- **Colour is per signal family**, shared by cable, plug and band, so a signal looks the
  same wherever it is drawn.

With thirty-two cables in a bay, legibility comes from what is dimmed: pointing at any
jack or cable drops the rest back far enough to follow one run, and arming a jack lifts
only the far row, which is the only place a patch can land.

An unrecognised routing byte has no jack to land on, so it is drawn as a dashed stub off
its channel rather than dropped — the value is real and round-trips.

## Measured against hardware

Firmware **1.3.1**. Everything in this section was observed on a real module, not derived
from the manual or the reference tool.

### Protocol

- **Config dump** is command `08H`, 768 words, 2313 bytes on the wire.
- **The macro region is two parallel arrays of 128 indexed by MIDI CC**: `[0..128]`
  levels, `[128..256]` pans. The config dump alone therefore drives the mixer UI; the
  `2AH` re-request the official tool relies on is unnecessary.
- **Pan is written to one place and stored in another.** It is written to CC
  `(m+1)*8 + ch` — the CC the right-channel mix would have used — but the module stores it
  in the **aux byte of the level cell** at `m*8 + ch`, as `written - 1`. Written centre is
  **64**, stored centre is **63**. A module whose pans have never been touched sits at
  stored 64, one step right of centre.
- **Macro drives raw.** A raw `0x60+mix` write sticks (`0x4000` written, `0x4000` read
  back) but is replaced the moment anything moves the corresponding macro control —
  including a CC arriving from a controller, which in the live rig is continuous. The raw
  matrix is therefore **read-only** in the UI.
- **The module talks back with more than an echo.** A single `34H` macro write draws an
  unsolicited full `11H` mix dump carrying the written value. A host needs a feedback
  guard, and send coalescing is a bandwidth requirement rather than a nicety.
- **`09H` chunked upload is reliable at full speed.** 24 chunks sent back to back with no
  pacing at all read back byte-identical. There is still no acknowledgement in the
  protocol, so callers verify by re-reading.
- **`26H` takes a parameter** — `1` hosted defaults, `0` standalone. It resets the
  *current* configuration and leaves both flash slots alone.
- Reserved dump words 719–767 read all zero.
- **Destinations sum.** Several mixes may target the same output and are added there
  (manual p.11), which is how both mixer banks can feed the main outs. A mix, by
  contrast, has exactly **one** destination.
- **Block 3's capture side feeds the S/PDIF output**, not the reverse. Its sources should
  be DAW channels 5–6; pointing them at `Source::Spdif` turns the optical output into a
  passthrough of the optical input.
- The published macro → raw curve is an **approximation** of what the firmware runs.
  Composing the documented curves reproduces unity exactly (macro 103 → `0x2000`) but is
  ~0.07 dB out elsewhere (macro 100 computes 6889; the module settled at 6832). Nothing
  depends on it — the module does the real conversion and reports the result — so it
  affects only the fidelity of the offline mock.

### DC blocking

Seven bits, one per **pair of adjacent ADC channels**. Because input numbers are permuted
on the wire, the pairs are not the obvious ones:

| Bit | 0 | 1 | 2 | 3 | 4 | 5 | 6 |
|---|---|---|---|---|---|---|---|
| Inputs | 1/2 | **3/8** | 4/5 | 6/7 | 9/10 | 11/12 | 13/14 |

A single-bit walk round-trips for all seven; writing `0xFF` reads back `0x7F`.

The filter must be **off** for CV — it removes exactly the constant offset a CV consists
of. Turning it on for audio on input 8 also turns it on for input 3.

### Audio

- **ASIO is the metering path**, on clock integrity rather than latency. Nothing here is
  in the signal path; the ES-9 mixes in hardware and the host only observes.
- The driver advertises 16 channels at **every** rate the module supports. 16 channels at
  48 kHz, ≈2.7 ms blocks, and it restores the module to 48 kHz rather than imposing a rate.
- **The driver is multi-client in practice: metering runs while Ableton is open on the
  same module.** The mixer is used this way on the rig.
- **WASAPI is rejected.** Shared mode can only run at the Windows endpoint's configured
  format, and opening a meter stream **re-clocked the module from 48 kHz to 32 kHz**,
  where it stayed until ASIO restored it. An explicit 48 kHz request was refused outright.
  A tool that silently changes the rig's clock to draw its meters is not acceptable.
- Peak and RMS both discard sign, so a channel resting at a constant voltage reads as
  silence on both. The Meters tab carries a **DC column** fed by a smoothed mean. On the
  attached module this corroborates the blocking filters exactly: inputs 3–8 (blocking on)
  read −0.0000, inputs 9–14 (blocking off) show real offsets between +0.0034 and −0.0049.

### MIDI on Windows

The ES-9 is published through **Windows MIDI Services** (`SWD\MIDISRV\MIDIU_KSA_...`)
under a **generic jack name** — `2 - MIDI In` / `2 - MIDI Out`, containing no "ES-9".
`midir`'s default WinMM backend does not enumerate it at all, and its `winrt` backend does
not change the list.

**The module cannot be found by name.** The shell opens every plausible input/output pair
and sends a version request; whichever answers is the module. Command-line tools take
explicit `--in`/`--out` indices.

## Invariants

Load-bearing decisions. Breaking one of these reintroduces a class of bug rather than a
single defect.

- **`DeviceState::apply` is the only mutation point.** It changes the model *and* emits
  the messages that make the module agree, together. Nothing can change the device without
  the model and the monitor seeing it.
- **Undo is by inverse action, not snapshot.** Each action returns the action that
  reverses it, so undo re-emits real SysEx and the device follows.
- **`Session::begin_edit` is the only way to change edit target**, and it restores the
  slot first. Connecting over USB forces hosted mode, so editing the standalone config
  without restoring silently edits hosted state instead. A test asserts the restore is the
  *first* message sent. This is the easiest way to lose a live configuration.
- **Routing is stored as wire bytes, not decoded enums**, so an unrecognised value
  survives a load/save cycle unchanged instead of being silently dropped.
- **A firmware 1.2 dump is rejected, not parsed.** Discriminate on the **command byte**:
  the `version` field is `3` in both formats, so it cannot detect the change.
- **Requests go one at a time.** The protocol has no flow control and a dump is 2313
  bytes; firing the startup requests together loses the dump.
- **The DC pair table is derived from `INPUT_CAPTURE_LOOKUP` by a test**, so it cannot be
  "tidied" into sequential pairs without failing.
- **Mixer strips are derived from the resolved CC map, not from the configuration**, so
  every fader carries the CC the hardware will honour and the stereo rules are applied in
  exactly one place.
- **The module's echo of the host's own writes is guarded, and the guard is bounded.**
  Every `34H` macro write draws a *full* `11H` mix dump, so a fader drag produces a stream
  of dumps trailing the pointer, each carrying the value of an earlier step. Folding them
  in verbatim walks the fader backwards under the hand holding it. `state::EchoGuard` pins
  a written byte to what the host wrote until the module echoes that exact value back —
  and releases the pin as soon as no write is outstanding, because a CC from a controller
  draws a dump too and a pin that only a matching echo could clear would freeze a fader
  the first time a write went missing. A full config read clears every pin.
- **The guard pins the byte the *view* reads, not the one the value was addressed to.**
  For pan those differ: it is written to the pan CC and stored in the aux of the
  corresponding level cell. `apply` therefore writes that aux byte itself, which is also
  what gives pan an optimistic local value instead of one that waits on a round trip.
  The raw matrix is deliberately **not** pinned — the host never writes the raw value a
  macro write produces, so there is no expected value to acknowledge, and the read-only
  raw readout does walk backwards for the length of a drag.
- **Nothing re-renders while a pointer is down.** A render rebuilds the panel, which
  destroys the very `<input>` the pointer is captured by and ends the drag — the reason a
  click on a fader used to stick when a drag did not. `flushSends` still sends every
  coalesced frame but defers its refresh, and the release flushes any last queued move
  before reconciling once.
- **`Applied.cc_changes` is attached to every action.** A stereo link toggle reports its
  sixteen rewritten CCs automatically, wherever it was triggered from.
- **`es9-app` is deliberately not a workspace member.** Tauri pulls GTK and dbus, which
  WSL2 lacks and the app could never use. Build it by manifest path on Windows.
- **Destructive actions arm on the first click and fire on the second.** Preset load and
  factory reset replace the entire configuration.
- **An output with no mix routed to it ignores its DC offset**, silently — the module
  implements offsets with the mixer hardware. `view::dc_offsets` computes this per output
  and the UI marks the inert ones.
- **A mode bit is not a routing statement**, and the UI must not report one as the other.
  Options bit 0 says block 3 *is* the S/PDIF processor; it does not say either direction
  carries anything. `view::status` therefore reports what the S/PDIF output is fed from
  and whether any capture channel listens to the input, and the header shows both. Same
  rule as the DC offsets above: the mode is not the routing.
- **Three kinds of "preset" are named apart, and must stay that way.**

  | Name | What it is | Reaches a module? |
  |---|---|---|
  | **Factory defaults** | The module's own, applied by `26H` | Yes — the module applies them itself |
  | **Mixer defaults** (`es9-device::mixer_defaults`) | This project's opinionated pair | **No** — mock and prototype only |
  | **User presets** (`es9-app::presets`) | Saved `08H` dumps as `.syx` on disk | Yes, via `09H` upload |

  The desktop app's reset sends `26H` and re-reads, so editing `mixer_defaults` changes
  mock fidelity and nothing else. Conflating the first two is how someone concludes a
  code change altered their rig, or the reverse.
- **`mixer_defaults::hosted()` deliberately deviates from the factory default.** USB
  capture 15/16 carry **MIX 1/2** rather than the S/PDIF input, giving the DAW a stereo
  reference recording of exactly what leaves the main outs, sample-aligned with the 14
  multitracks. The cost is that the S/PDIF input no longer reaches the host. This is a
  trade, not an error — do not "correct" it back without knowing which side is wanted.
  Note the consequence on hardware: **a factory reset replaces routing too**, putting
  S/PDIF back on capture 15/16 and removing any reference capture set up there. The
  Presets tab says so.

- **The patchbay never invents a jack.** Its rows come from `view::available_sources` /
  `available_destinations` and its cables from the wire bytes in `view::routing`. A wire
  value with no matching jack is drawn as a stub, never silently omitted — the same reason
  routing is stored as wire bytes rather than decoded enums.
- **Both rows of a bay share one jack pitch and one origin.** The vertical alignment
  between Input *n* and USB in *n* is the affordance that makes a non-obvious routing
  visible as a diagonal. Adding a gap between families in either row destroys it; the
  labelled bands exist precisely so no gap is needed.

⚠️ **`frontendDist` is baked into the binary at compile time.** Editing `web/` requires
rebuilding the shell, or you are looking at the previous UI. The symptom — correct data in
the backend log, stale data on screen — looks exactly like a state bug. A rebuild is now
enough to pick it up: `crates/es9-app/build.rs` watches the directory itself, because the
`rerun-if-changed` that would do it lives in `tauri-build`'s `codegen` feature and this
crate does not enable it. Without that, cargo saw no reason to recompile a crate whose
Rust had not changed, `tauri::generate_context!()` never re-expanded, and a *freshly
built* binary served the old UI. `build.rs` asserts the watched path is a real directory,
so it cannot drift from `tauri.conf.json` and reintroduce the trap silently.

⚠️ **Nothing touching hardware can be tested from the Linux side.** WSL2 has no sound
subsystem and no USB passthrough. Windows `cargo` builds directly from the `\\wsl$` path,
so the working copy stays in WSL2 and only artefacts are Windows-side.

## Not verified

Stated as gaps rather than assumptions, so nothing downstream treats them as established.

- **Which physical inputs each DC bit governs** is inferred from the input permutation,
  not measured. Confirming it needs a DC offset on one input of a pair, the filter
  toggled, and the signal observed.
- **The physical input and output permutations** decode plausibly (Input 1–14, S/PDIF,
  buses, ES-5, main/phones) but have not been confirmed by ear against what is actually
  patched. This includes the `Output 7 = 11` / `Output 8 = 10` swap.
- **The pan half of the macro region** (`[128..256]`) is inferred from a single
  configuration in which all 128 words read the pan centre. It stays written verbatim
  until a deliberately asymmetric pan configuration confirms it the same way the level
  half was confirmed.
- **Loading a preset and the factory reset have not been pressed against hardware.** Both
  replace the live configuration. Both are covered by tests through the mock, and the
  `09H` upload transport they rely on is verified, but the buttons themselves are untried
  on the module.
- **Whether routing and option changes are echoed** the way macro writes are.
- **The patchbay has never been seen in a browser.** Its geometry, hit testing and write
  path are exercised headlessly against the mock — jack and cable counts, the family
  bands, patching from either end, the no-op when a connection already exists — but that
  harness is jsdom, which has no layout engine, and it lives outside the repo because the
  project otherwise has no npm dependency. Nothing has confirmed how the bay *looks* at a
  real window size, how a thirty-two-cable bay reads in motion, or that the hover dimming
  does the work it is relied on to do.
