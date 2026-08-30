# ES-9 Mixer UI — Development Log

## 2026-08-28 — Protocol decode

Pulled `es9_config_tool_1.2.html` from expert-sleepers.co.uk and archived it at
`docs/reference/`. The tool is unminified, unobfuscated JavaScript with the entire
protocol in plain sight — no traffic capture or binary analysis was needed. Decoded it
into `docs/es9-sysex-protocol.md`.

**What the protocol turned out to be:** `F0 00 21 27 19 <cmd> ... F7`, with `00 21 27`
as Expert Sleepers' manufacturer ID and `0x19` identifying the ES-9. Roughly 30 commands.
Wide values are big-endian 7-bit groups; signed fields sign-extend from 16 bits.

**Key structural finding:** the device is four 8-channel routing blocks. Blocks 0 and 1
are the 16 USB capture/playback channels. Blocks 2 and 3 are the mixer — each provides 8
shared source channels feeding 8 mixes, giving the 16 x 8 matrix. Block 3 is dual-purpose:
options bit 0 switches it between S/PDIF ports and a second mixer bank.

**Second finding:** the raw 21-bit gain matrix and the 7-bit level+pan "virtual" mixer
are *both* device-side. The mix dump returns both, and the tool never derives one from
the other. This is the main thing v1.2 presents without explanation, and it drives open
decision D1.

**Defects found in the reference tool**, recorded in the protocol doc §8 so they are not
reinherited. The notable ones: `send()` calls an undefined `makeSysEx()`; pan is written
to `0x34` row `mix+1` but read back from row `mix`, so it cannot survive a reload; and
the pan bias disagrees between send (`64 + pan`) and receive (`byte - 63`).

**Decisions taken:**

- Target: local single-file WebMIDI app, matching the reference tool's delivery model.
- Scope: mixer and routing. EQ, DC offsets, codec control and MIDI config are documented
  but not built.
- No hardware on hand, so the protocol layer is built against a mock device and every
  unverified assumption is isolated and listed for later checking.

**Open:** D1–D4 in `requirements.md` §8 — raw vs virtual mixer, routing presentation,
visual density, and whether the absence of metering is acceptable.

## 2026-08-28 — Decisions and plan

Resolved D1–D4:

- **Mixer model:** virtual (level + pan) primary, raw 21-bit matrix behind an advanced
  disclosure. This promotes protocol open question 7.6 to blocking — if a raw write is
  clobbered by the stored virtual value, the advanced panel is a lie. Tracked as H6.
- **Routing:** patchbay matrix. Worth noting the underlying model is not a free matrix:
  each capture and output channel takes exactly one assignment, so every column has at
  most one active cell and clicking is a *move*, not a toggle. That has to read correctly
  in the interaction or the patchbay will mislead.
- **Density:** console layout, one mix in focus.
- **Metering:** none. The protocol has no audio metering, only DSP load.

Wrote `plan.md`. The one structural decision there: the single-file deliverable is built
from `src/` by a dependency-free `build.mjs`, rather than being authored as one file.
Authoring in one file would make the protocol layer untestable and repeat v1.2's
DOM-as-state-model mistake — and with no hardware to check against, the encode/decode
round-trip test is the only thing standing between a misread offset and silent corruption.

Hardware checklist has 11 items, 7 blocking. H1 (dump size/version) and H3/H4 (pan row
and centre bias) are the ones most likely to invalidate current assumptions.

## 2026-08-28 — Metering research; protocol doc substantially corrected

Went looking for metering. Found something more valuable on the way.

### The reference tool I started from was a version behind

There is a **firmware 1.3** and a matching config tool, and — more importantly — an
**official MIT-licensed source repo**: `expertsleepersltd/ES-9_tools`, containing
`es9_config_tool.html` for firmware 1.3.0. Copyright (c) 2023 Expert Sleepers Ltd, MIT.
So this was never a black box, and we can derive from the official source directly.

The v1.3 **user manual also contains a complete SysEx appendix** (pages 15–19). That is
now the primary source for `docs/es9-sysex-protocol.md`, with the tool source as
cross-check.

### The dump format changed between firmware 1.2 and 1.3

The single most consequential finding. Firmware 1.2 sends command `10H` with a mixed
byte/word layout. Firmware 1.3 sends command `08H` with a **uniform array of 21-bit
words** — every field, including single-bit flags, takes three bytes. Different command,
different offsets, different MIDI-channel packing, and 1.3 additionally carries the macro
mix in the dump.

The `version` field is **still 3 in both**, so the tool's version check cannot detect the
difference. Discriminate on the command byte. Had I built against the 1.2 layout — which
is what the plan said to do — every offset past the routing block would have been wrong
on current firmware, and the version guard would have passed silently.

### Corrections to the protocol doc

- Raw mix is **16-bit**, not 21-bit. 21 bits is just the transport encoding.
- "Virtual mix" is officially the **macro mix**. Smoothing is documented as interpolating
  the macro→raw conversion, which establishes that **macro drives raw** — promoted to Q5,
  because it may mean a direct raw write is transient and the planned "advanced raw
  panel" is not meaningful.
- `32H` inbound is a general NUL-terminated **Message**, not just a version string; it
  also carries operation feedback such as `"OK"` after a flash save.
- The two mystery bytes after the dump command are documented as literal `00 00`.
- New command found: **`09H`**, chunked config upload — 24 chunks of 32 words, no
  acknowledgement.
- Mixer is an **8x8 crosspoint** per block; USB block is 16x16; 16 internal summing buses.

Open questions dropped from 7 to 8 but changed character: they are now genuine gaps in the
official spec rather than gaps in my reading of it.

### Metering: not in the protocol, and the browser can't substitute

No metering command exists. Confirmed against the manual's complete command list, the 1.2
tool, and the official 1.3 tool. The only telemetry is DSP load, which is processing cost,
not audio level.

The obvious workaround is to meter the USB audio stream host-side — the ES-9 can route
inputs, buses or mix outputs to any of its 16 USB capture channels, so the host can
observe any point it will spend a capture channel on. But **Chrome clamps `getUserMedia`
audio input to 2 channels**; `channelCount > 2` raises `OverconstrainedError`. The
Chromium issue has been open since 2015 and Firefox has the same limitation.

So: a browser app can meter one stereo pair. Full 16-channel metering needs a native audio
path. This reopens the platform decision recorded on 2026-08-28.

### Prior art

- `odaacabeef/es9-mixer` — mixing-focused web UI, requires fw 1.3.0
- `trotttrotttrott/es9-mixer` — config-tool-derived web UI with mix hiding

Neither appears to do metering. Worth reading before building, not adopting wholesale.

## 2026-08-28 — Platform changed to Tauri; metering has a blocking unknown

Decisions: **Tauri desktop app**, Windows target (ES-9 is on the Windows host, WSL2 is
just the editor), metering covering **inputs and mix outputs**.

Chose Tauri over Electron because `cpal` has a maintained ASIO backend and the ES-9 needs
ASIO on Windows for more than stereo; the Electron path (`naudiodon`/PortAudio) needs a
native rebuild per Electron version and is less maintained. MIDI (`midir`), audio (`cpal`)
and the protocol codec then all live in Rust.

### The blocking unknown

The ES-9 uses an Expert Sleepers **ASIO** driver on Windows for multichannel, and ASIO
drivers are conventionally **single-client**. If this one is, the app cannot open the
audio device while a DAW has it — which is exactly when you want meters. Metering would
then only work with no DAW running, which guts the reason for leaving the browser.

Recorded as **spike S0** and risk R-0, and it is now the first thing to do when hardware
is available. Three outcomes, three different plans: multi-client ASIO → proceed; WASAPI
shared mode exposes all 16 → use WASAPI and drop the ASIO SDK dependency entirely;
neither → re-decide metering.

Added **spike S1** alongside it: whether a raw `0x60+mix` write survives, given the manual
says smoothing interpolates macro→raw. If macro continuously drives raw, D1's advanced
raw panel is not meaningful and gets dropped.

### Other consequences

- Firmware target narrowed to **1.3.x only**, detected by command byte rather than the
  version field, which is 3 in both formats.
- Build environment is now a real concern: repo in WSL2, artefact is a Windows binary,
  ASIO SDK is Windows-only. Logged as R-1 — needs settling before Phase 1.
- Hardware checklist shrank from 11 items to 9 plus the two spikes, because the manual
  answered most of the original questions.

Plan rewritten around six phases with Phase 0 as the spikes. Metering is Phase 5 and
explicitly gated.

## 2026-08-28 — Live use case clarified; MIDI CC map becomes core

The real goal surfaced: Ableton can route audio to the ES-9 but cannot control its
internal gain and routing. The wanted workflow is **preconfigure on the desktop, then
perform DAW-free with a MIDI controller driving the matrix mixer**. Both contexts — laptop
attached without a DAW, and fully standalone — are first-class.

### The ES-9 already does MIDI CC mixer control in hardware

No computer needed in the signal path. Enabled per-port with separate MIDI channels for
USB and DIN (or Off). It drives the **macro** mix, not the raw mix.

But the CC numbers are **fixed**: `CC = mix*8 + channel`, mixes 1–8 on CC 0–63, Mixer 2 on
64–127. And the stereo rules are subtle — use the lower CC of a stereo pair, and pan takes
the CC the right-channel mix would have used. **Toggling a stereo link therefore silently
rewrites what half the CC numbers mean.** A controller mapping is only valid for one link
configuration. That is exactly the kind of thing a tool should surface, so the resolved CC
map view became a core feature (D6, R26–R30) rather than a nicety.

### Q3 resolved, for free

The manual states the byte after `34` *is* the CC number. So the macro-mix parameter space
and the MIDI CC space are the same 128 entries — which is why there is no separate pan
command. Pan for a stereo pair sits at `(m+1)*8+ch`, confirming the config tool's write is
correct and its read is the broken half. One less hardware test.

### The flash-slot trap sits directly on this workflow

Connecting over USB forces the module into **hosted** mode. So "edit the standalone config"
requires restoring slot 0 first, editing, then saving back to slot 0 — and if you skip the
restore you silently edit hosted state instead. The reference tool does nothing to prevent
this, and it is the easiest way to lose a live configuration. Now a guarded single action
(R23, plan step 19b).

### Hardware constraint worth flagging

The ES-9's USB is a **device** port, so a USB MIDI controller cannot plug into the module.
CC control with no computer present requires the **DM4 DIN breakout**. Without it, fully
standalone gigs are static-preconfigured-mix only. Purchasing decision, not a software one
— logged as R-6.

### Consequences for metering

Risk R-0 downgraded from blocking. The primary live context is DAW-free, so ASIO
exclusivity costs meters only during hosted DAW sessions. S0 now only has to establish that
a 16-channel stream opens at all.

Also worth noting: Phase 4c (CC map) needs no audio at all and delivers value in the fully
standalone case. If S0 goes badly, that phase still stands.

## 2026-08-28 — Phase 1 protocol layer and CC map resolver built

DIN breakout confirmed as acquired, so R-6 is closed and both MIDI paths are in scope.

Built `crates/es9-protocol`: a pure Rust library, no I/O, no Tauri, no platform
dependencies. It compiles and tests in WSL2 today — the Windows/ASIO toolchain question
(R-1) only bites when audio and the shell get added, so it is not blocking anything yet.

**29 tests, clippy clean, rustfmt clean.**

### Modules

| Module | Contents |
|---|---|
| `frame` | header, 21/14-bit packing, sign extension, frame validation |
| `tables` | sources, destinations, signal families, the input permutation |
| `links` | the 32-bit stereo link bitmap, addressed per family |
| `scales` | raw ↔ dB (16-bit), macro ↔ dB (7-bit piecewise), EQ conversions |
| `config` | the 768-word firmware 1.3 dump model |
| `decode` | all inbound messages |
| `encode` | all outbound commands, including `09H` chunked upload |
| `ccmap` | resolved MIDI CC map |

### Decisions taken while building

- **Routing stored as wire bytes, not decoded enums.** An unrecognised value then
  survives a load/save cycle unchanged instead of being silently dropped. Decoded
  accessors sit on top.
- **The macro-mix region of the config dump is preserved verbatim** as raw words. Its
  internal layout is unestablished (Q1) because the official tool skips it entirely, so
  guessing would risk corrupting a field on write. Same reasoning for `MacroCell::aux` in
  the mix dump, which the reference tool reads as a biased pan but writes elsewhere.
- **A firmware 1.2 dump is rejected, not parsed.** `FirmwareTooOld` carries the reason.
  There is a test asserting this specifically, since the version field cannot detect it.
- **Frame parsing validates the 7-bit constraint** on payload bytes. A dropped or
  duplicated byte in a 2313-byte dump is cheapest to catch there.

### The CC map test that failed first

Asserted that linking mixes 1/2 leaves CC 0–7 alone, since those stay levels. Wrong: they
change from *mono mix 1* levels to *stereo pair* levels. **One link toggle rewrites all
sixteen CCs across both rows** — eight become pans, eight change what they span. The test
now asserts that, and it is a good argument for R28 existing at all: the surprise is
bigger than "some CCs became pans".

The manual's worked examples are transcribed verbatim into `tests/ccmap.rs`, since they
are the only authoritative statement of the stereo rules.

### Still open

Nothing in this layer is blocked. Q1 (macro word layout), Q2 (pan bias), Q4 (upload
reliability), Q5 (raw vs macro precedence) and Q6 (echo) all need hardware, and all are
isolated behind either preserved-verbatim data or a single code path.

## 2026-08-28 — Phase 2 complete: state, actions, undo, mock, transport

Three crates now, **57 tests**, clippy and rustfmt clean. Everything still runs with no
hardware attached.

| Crate | Role |
|---|---|
| `es9-protocol` | pure codec (Phase 1) |
| `es9-device` | state, actions, undo, offline mock, send coalescing — pure |
| `es9-midi` | `midir` transport, SysEx reassembly, protocol monitor |

`midir` builds fine in WSL2; ALSA headers were already present. R-1 (the Windows toolchain
split) still stands but does not bite until Tauri and audio arrive.

### Design decisions

- **One choke point.** `DeviceState::apply` mutates the model *and* emits the messages
  that make the module agree, together. Nothing can change the device without the model
  and the monitor seeing it, which is N4 satisfied by construction rather than convention.
- **Undo by inverse action, not snapshot.** Each action returns the action that reverses
  it, so undo re-emits real SysEx and the device follows. A `Batch` inverts as its
  members in reverse order and undoes as one step.
- **`Applied.cc_changes` on every action.** The CC map is recomputed before and after and
  the diff is attached. A stereo link toggle therefore reports its sixteen rewritten CCs
  automatically, wherever it was triggered from — R28 falls out rather than needing UI
  discipline.
- **`Session::begin_edit` is the only way to change edit target.** It restores the slot
  first, then requests config and mix. The trap from the manual — that connecting over USB
  forces hosted mode, so editing standalone without restoring silently edits hosted state
  — is now structurally impossible. There is a test asserting the restore is the *first*
  message sent.

### Things the build surfaced

- **A config dump is 2313 bytes**, two orders of magnitude beyond a normal MIDI message.
  Whether a backend delivers that in one callback is not guaranteed, so `SysexAssembler`
  buffers to the terminating `F7`. Tested by feeding a real dump through in 64-byte
  fragments.
- **Realtime bytes can appear inside a SysEx message.** MIDI clock and active sensing may
  interleave anywhere, so the assembler discards them rather than letting them corrupt a
  payload. Tested with clock and active-sensing bytes injected mid-message.
- **Echo behaviour is still unknown (Q6), so the mock makes it a flag** rather than
  picking an answer. There is a test running with echo on, confirming a client absorbs the
  echo without looping. When hardware settles this, only the flag's default changes.
- **The monitor decodes both directions**, and outgoing `34H` renders as
  `"macro mix: CC 8 = 64"` — the manual's own recommended method for identifying a
  control's CC number, now a labelled feature rather than a debugging trick.
- **The coalescer takes time as a parameter** rather than reading a clock, so drag
  behaviour is deterministic under test. `flush_now` exists so a drag ending mid-interval
  cannot strand its final value.

### Still open

Unchanged: Q1, Q2, Q4, Q5, Q6 and spikes S0/S1 all need hardware. None block the UI.

## 2026-08-28 — WASM bridge and browser prototype

Chose the WASM-first route so the UI is developable entirely in WSL2, with Tauri and the
Windows toolchain deferred until something actually needs native access.

**68 Rust tests plus an end-to-end headless smoke test**, clippy and rustfmt clean.

### What went in

- `crates/es9-wasm` — a `Bridge` exposed to JavaScript, driving the offline mock. Its
  method surface is deliberately the shape the native backend will present, so swapping
  the mock for `midir` should not change a line of frontend code.
- `es9-device::view` — the presentation model: mixes, strips, CC rows, patchbay, labels.
  Kept in Rust rather than the frontend so the labelling and stereo-collapsing rules are
  testable and identical in both builds. 11 tests.
- `es9-device::presets` — the manual's two default patches. The standalone one (analogue
  inputs to the mixer, Mixer 2 on) is what the prototype boots into, so the interface
  shows a real rig rather than an empty matrix.
- `web/` — console mixer, patchbay, CC map and protocol monitor.

### Decisions

- **Mixer strips are derived from the resolved CC map, not from the configuration.** That
  means every fader carries the CC number the hardware will honour, and the stereo rules
  are applied in exactly one place. Pairing levels with pans became
  `CcMap::strips()`.
- **`Monitor` moved from `es9-midi` down into `es9-device`**, with timestamps supplied by
  the caller instead of `Instant::now`, which panics on wasm32. Same reasoning as the
  coalescer. `es9-midi` now supplies elapsed milliseconds; the browser supplies a counter.
- **The raw matrix is behind a disclosure that states it is unverified.** D1 asked for
  "advanced", but until spike S1 says whether a raw write survives the macro layer, it
  would be dishonest to present it as an ordinary control. It carries the caveat inline.
- **A link toggle raises a banner listing every rewritten CC.** The model already computed
  the diff, so this cost nothing and is the clearest expression of R28.

### Notes

- `wasm-bindgen` CLI and crate versions must match exactly. Cargo resolved 0.2.127 against
  a 0.2.104 CLI and the build failed with a schema mismatch; updated the CLI rather than
  pinning the crate backwards.
- The smoke test (`./scripts/smoke.sh`) exercises the whole stack under Node: preset
  wiring, a fader round trip through the mock, undo, a link toggle reporting sixteen CC
  changes, strip merging, patchbay assignment, and the guarded standalone workflow —
  asserting from the monitor log that the restore really was sent first.

### Housekeeping

Initialised a git repository (there was none). Nothing has been committed or staged.

### Still open

Unchanged: Q1, Q2, Q4, Q5, Q6, and spikes S0/S1 all need hardware. The prototype is a
prototype — no native MIDI path is wired up yet, and no audio.

## 2026-08-30 — Hardware attached; S0 resolved, most of the checklist answered

An ES-9 is connected. Everything below is measured against the real module rather than
the mock, so this entry supersedes assumptions, not adds to them.

### The build environment question (R-1) settled itself

WSL2 has no sound subsystem and no USB passthrough — no `/proc/asound`, no `lsusb`. The
module is on the Windows host and is not reachable from the Linux side at all. But the
Windows host already had everything needed: rustup `stable-x86_64-pc-windows-msvc`, VS
Build Tools 2022 and LLVM. Windows `cargo` compiles directly from the `\\wsl$` path, so
the working copy stays in WSL2 and only the artefacts are Windows-side. Captured in
`scripts/win-build.ps1`. **R-1 closed** — no cross-compilation, no second checkout.

### The ES-9's MIDI is invisible to the backend we were using

`midir`'s default Windows backend is WinMM, and WinMM enumerated only virtual ports —
no ES-9. The module is published as a `MidiEndpoint` under `SWD\MIDISRV\MIDIU_KSA_...`,
which is **Windows MIDI Services**, the new stack. Switching `midir` to its `winrt`
backend on Windows targets did not change the list either.

What actually found it: the port is there, but named **`2 - MIDI In` / `2 - MIDI Out`** —
a generic jack name with no mention of "ES-9". So `find_es9`'s name matching cannot work
on Windows, and the probe grew explicit `--in`/`--out` indices. **A port picker, and
probably an identify-by-handshake, is now a requirement rather than a nicety** — the app
cannot assume it can find the module by name.

### Checklist results

| # | Check | Result |
|---|---|---|
| H1 | Dump is 768 words, command `08H` | **PASS.** 2313 bytes, cmd `0x08`, exactly 768 words. Decoded first time. |
| H2/Q1 | Macro-mix word layout | **Answered.** See below. |
| H4/Q2 | Pan centre bias, 63 or 64 | **64.** Every resting `aux` reads 64. The reference tool's `byte - 63` read really is wrong. |
| H9 | Reserved words 719–767 | All zero. |
| S0 | Multichannel capture | **Resolved.** See below. |

Firmware reports **1.3.1**, so the 1.3 target is correct.

**Q1 is answered, and cheaply.** The 256-word macro region is two parallel arrays of 128
indexed by CC: `[0..128]` levels, `[128..256]` pans. The evidence is direct — word 0 and
word 9 both read 103, and the mix dump independently reports CC 0 and CC 9 at 103, while
all 128 words of the high half read exactly 64, the pan centre. That means the config
dump alone can drive the mixer UI, and the `2AH` re-request the official tool relies on
is not actually needed. The region can stop being preserved-verbatim-and-opaque.

One caveat worth keeping: this is inference from a single configuration, so the layout
stays preserved verbatim on write until a second, deliberately asymmetric configuration
confirms it.

### S0: metering works, and the answer changed the platform decision back

**WASAPI exposes all 16 capture channels.** That looked like the best possible outcome —
the row in the plan that says "drop the ASIO SDK dependency entirely". It is not.

WASAPI shared mode can only ever run at the Windows endpoint's configured format. The
endpoint was set to **32 kHz**, so that is the only rate it offered, and an explicit
request for 48 kHz was refused outright: *"The requested stream configuration is not
supported by the device."* Worse, the module reported **48000 Hz before** the first
WASAPI stream was opened and **32000 Hz after** — opening the meter stream re-clocked the
rig. It stayed at 32 kHz until ASIO touched it, at which point it returned to 48 kHz.

For a tool whose whole purpose is to avoid silently changing a rig, metering that drags
the module to whatever Windows has in a dropdown is disqualifying. Latency was never the
issue — nothing here is in the signal path, the ES-9 mixes in hardware and the host only
observes — but **clock integrity** is.

**ASIO is therefore the metering path, on its merits rather than for latency.** Using the
Steinberg SDK's GPLv3 option via `github.com/audiosdk/asio`:

- The driver advertises 16 channels at **every** rate — 32k, 44.1k, 48k — not just one.
- Opened 16 channels @ 48 kHz, 1499 callbacks in 4 s (≈2.7 ms blocks), live meters on 14.
- It restored the module to 48 kHz rather than imposing a rate of its own.

**Risks R-0 and R-5 are closed.** R-5 (ASIO SDK friction) turned out to be nothing: LLVM
was already installed and `asio-sys` built without incident.

Licensing consequence, recorded rather than glossed: taking the GPLv3 option makes
`crates/es9-audio` and anything linking it GPL-3.0. That crate's manifest now says
`GPL-3.0-only`; the four pure crates stay MIT.

### Still open

- **R-0 was not actually tested.** No DAW was running during the ASIO spike, so whether
  the driver is multi-client is still unknown. The plan already downgraded this because
  the primary context is DAW-free; it stays downgraded and untested.
- **H3, H5/Q6, H6, H7, H8, S1** all need either a deliberate write or an ear on the
  outputs. The probe has the write-side spikes behind `--write-tests` and they have not
  been run — a dump is archived first, per §8.
- H6/H7 printed a plausible routing table (Input 1–14, S/PDIF, buses, ES-5, main/phones)
  but "plausible" is not "verified against what is physically patched".

### Also

`es9-audio` is a new crate: `meter.rs` with peak/RMS ballistics and peak hold, 9 tests.
Writing the tests caught a real defect — the peak decayed when a block's peak *equalled*
the running peak, so a steady tone drooped instead of holding. `>` became `>=`.

**77 tests, clippy and rustfmt clean.**

## 2026-08-30 — Write-side spikes run; DC blocking built

Ran the write-side hardware tests and built input DC blocking, which is a scope addition:
the original plan explicitly excluded it ("EQ, DC offsets, codec control and MIDI config
are documented but not built"). It is needed for the actual rig, so it is in.

### Write-side results

**S1 — macro drives raw. Confirmed, and it settles D1.** A raw `0x60+mix` write *does*
stick: wrote `0x4000`, read back `0x4000`. But a `0x34` macro write to the same cell then
replaced it. So a raw value survives only until anything moves the corresponding macro
control — including a CC arriving from a controller, which in the live rig is constant.

**The advanced raw-matrix panel (plan step 15) should be dropped.** Presenting a control
whose value silently will not survive normal use is worse than not offering it. R-2
resolves the way it was feared to.

**Q6 — the module talks back, and with more than an echo.** A single macro write drew an
unsolicited **full `0x11` mix dump** carrying the written value. Two consequences: a
feedback guard is genuinely required, and a fader drag emits a large dump per step, so the
coalescer built in Phase 2 is a bandwidth requirement rather than a nicety. That is a
better outcome than the flag-based guess in the mock — only the mock's default changes.

**H10 — DC blocking round-trips.** Walking a single bit through the field round-tripped
for all seven bits. Writing `0xFF` read back `0x7F`, confirming a seven-bit field. The
module's filters were all on; restored.

The probe now restores what it disturbs. First run left macro CC 0 at 100 instead of 103,
because it restored the raw cell but not the macro — and since macro drives raw, that
restore was meaningless anyway. It now saves the macro value from the dump, and puts the
macro back *before* the raw cell for the same reason. `--set-macro <cc> --to <v>` exists
so a stray value can be put back without re-running the suite, which would capture the
already-changed value as the "original". CC 0 is back at 103.

### The DC-blocking pairing is the interesting part

The code said "bits 0-6 for inputs 1-7". That is wrong, and it would have produced a UI
that lies. The manual (p. 9): the filters are "switched on or off in pairs". Fourteen
inputs, seven pairs, seven bits.

The pairs are not the obvious ones. From the official tool's own labels:

| Bit | 0 | 1 | 2 | 3 | 4 | 5 | 6 |
|---|---|---|---|---|---|---|---|
| Inputs | 1/2 | **3/8** | 4/5 | 6/7 | 9/10 | 11/12 | 13/14 |

"3/8" looks like a typo in the reference tool. It is not. A filter covers two adjacent
**ADC channels**, and input numbers are permuted on the wire by `INPUT_CAPTURE_LOOKUP` —
which this codebase already had. Map each pair through that table and every one comes out
as two wire bytes differing only in the low bit, 3/8 included: `0x76`/`0x77`.

That derivation is now a test rather than a comment, so the table cannot be "tidied" into
sequential pairs without failing. Which physical inputs a bit governs is still *inferred*
— corroborated by the permutation, but not measured with a CV on an input.

**Why this matters enough to have spent the time:** the filter must be **off** for CV,
since it removes exactly the constant offset a CV consists of. Someone turning the filter
on for audio on input 8 also turns it on for input 3. A seven-checkbox UI labelled
"inputs 1-7" — which the wrong comment would have produced — would silently destroy a CV
patch and give no clue why.

### What got built

- `es9-protocol::dcblock` — pair table, bit/input mapping, labels. 7 tests, including the
  ADC-adjacency derivation.
- `Action::SetDcBlock { bit, on }` — separate from `SetHpf(bitmap)` so undo restores one
  filter rather than the whole field, and callers do no bit arithmetic.
- `view::dc_filters` — carries a `surprising` flag for any non-adjacent pair, so the UI
  does not have to know which one it is.
- The monitor decodes `0x31` by name: `"DC blocking: Inputs 1/2, Inputs 3/8"` rather than
  a bare bitmap.
- An **Inputs** tab. Each card states what the setting *means* — "filtered — for audio" /
  "unfiltered — passes CV" — because "on" and "off" do not tell you which one breaks a CV.
  The 3/8 card carries its own warning inline.
- Presets now boot with all filters on, matching what the module was found holding and the
  manual's advice for audio. The manual does not state a default, so that is recorded as
  an observation rather than a documented value.

Corrected the misleading doc comments on `Config::hpf` and `encode::set_hpf`, and added
§10 to the protocol doc.

**88 tests, clippy and rustfmt clean, smoke test passing.**

### Two smaller things the spikes turned up

**The mock's echo flag now defaults to on, and does something different.** It had
`echo_writes: false` and echoed raw writes. The hardware's actual behaviour is a full
`11H` mix dump after a *macro* write, so the mock now does that, and the quiet case is
what has to be asked for rather than the default. The mock also follows macro → raw on a
`34H` write, which it previously did not model at all.

**The published macro → raw curve is an approximation (new Q9).** Composing the documented
curves reproduces unity exactly — macro 103 → `0x2000`, matching hardware to the digit —
but macro 100 computes 6889 where the module settled at 6832, about 0.07 dB out. Nothing
depends on it, since the module does the real conversion and reports the result; it
affects only mock fidelity. Recorded as a bounds test rather than an equality, with an
assertion that fails if it ever starts matching exactly, so the note cannot rot silently.

### Still open

- **Which inputs a DC bit actually governs** is inferred, not measured. Needs a DC offset
  on one input of a pair, filter toggled, signal observed.
- H3 (pan row for a stereo pair), H6/H7 (routing permutations by ear), H8 (`09H` upload
  reliability) still need hardware attention.
- R-0 (ASIO alongside a DAW) still untested.

## 2026-08-30 — The desktop shell exists and runs on the module

`crates/es9-app`: a Tauri shell joining the three layers built separately — the device
model and undo history, the MIDI transport, and audio metering. It connects to the real
ES-9, shows its actual configuration, and meters sixteen channels at 48 kHz over ASIO.

### Nothing was actually in the way

The blockers I expected were not blockers. WebView2 is already installed. Neither the
Tauri CLI nor Node is needed: a Tauri app builds with plain `cargo build` given a
`build.rs` and a `tauri.conf.json`, and the frontend is static so it needs no bundler.
What was needed was an icon and a `capabilities/` file, both of which the build script
asks for by name.

**Tauri will not build on the Linux side** — it pulls GTK and dbus, which WSL2 does not
have and which the app could never use anyway. So `es9-app` is deliberately **not a
workspace member**; it is its own workspace, excluded from the root. The Linux test loop
stays fast and honest, and the shell is built by manifest path on Windows.

### One frontend, two backends

The browser prototype and the desktop app now share `web/` rather than forking it.
`web/backend.js` picks at load time: Tauri's `invoke` if it is running in the shell, the
WASM mock otherwise. Both are presented as the same async interface, so `app.js` does not
know which it is talking to. That cost an async pass over the frontend — every backend
call is awaited now, and `refresh()` fetches everything a render needs up front so no
render function has to be async.

### Finding the module by asking, not by name

Windows publishes the ES-9 through Windows MIDI Services under a generic jack name with
no "ES-9" in it, so name matching cannot work. The shell opens every plausible
input/output pair and sends a version request; whichever answers is the module. It found
input 1, output 2 on the first run and needs no configuration.

### Requests must go one at a time

The first version fired all four startup requests together and the configuration dump
never arrived — the UI showed defaults while the header showed a real version, which is
exactly the sort of half-truth this tool must not tell. The protocol has no flow control
and a dump is 2313 bytes. Requesting serially and waiting for each reply is what the
hardware probe had been doing all along; the shell now does the same, and says so loudly
in the log if the dump does not arrive.

### Three real bugs, found by looking at the thing

Screenshotting the running app was worth more than any amount of reasoning about it.

**Pan was hard left on every strip.** The view read pan from the value of the CC pan is
written to. That CC always reads 0, which renders as full left. A sweep on hardware
settled it: pan is written to CC `(m+1)*8+ch` but **stored in the aux byte of the level
cell**, as `written - 1`.

This retracts two things this project had written down. The reference tool was recorded
as buggy here — writing `64 + pan` and reading `byte - 63` — and it is not: those compose
exactly, because the device subtracts one in between. And the earlier conclusion that
"pan centre is 64" was half right: 64 is the *written* centre, 63 is the *stored* one, and
a module whose pans have never been touched sits at stored 64, one step right of centre
rather than at it. Both constants now exist, `docs/es9-sysex-protocol.md` §11 has the
measured sweep, and §9 carries the retraction so the tool is not libelled twice.

**Tab switching never worked.** `#panel-mixer { display: grid }` is an ID selector and
outranks `.panel[hidden]`, so the mixer panel could not be hidden and every other tab
rendered underneath it. This was wrong in the browser prototype too and had never been
noticed, because nobody had looked at a tab other than the first.

**The initial tab came from the markup, not from the state.** `selectTab` was only ever
called from a click, so the `tab` variable and the visible panel agreed by coincidence.
Startup now drives the panel from the variable.

### A mistake worth recording

I spent a while debugging a frontend that was not the one running. Tauri bakes
`frontendDist` into the binary at compile time, so editing `web/` and re-running the old
executable shows the old UI. The symptom — correct data in the backend log, stale data on
screen — looked exactly like a state bug. **Editing the frontend requires rebuilding the
shell.**

### Metering, confirmed by eye

The Meters tab shows sixteen bars labelled from the routing, so a meter says what it is
carrying rather than just that something is there: Input 1–14 and S/PDIF L/R, matching the
capture blocks exactly, with live signal on inputs 3–8.

### Still open

- **R-0 remains untested.** Ableton was running during these sessions and the ASIO stream
  opened anyway, but whether Ableton actually held the ES-9 driver at the time was never
  established, so this proves nothing and is not being counted.
- H6/H7 (routing permutations by ear), H8 (`09H` upload), H11 (which inputs a DC bit
  governs) still need deliberate hardware attention.
- The shell has no port picker yet; autoconnect covers the common case but there is no
  way to override it from the UI.

**96 tests, clippy and rustfmt clean, smoke test passing.**

## 2026-08-30 — Routing split, output DC offsets, DC metering

Three related pieces of work, prompted by using the app on the rig.

### The routing tab had nested vertical scrolling

The Routing tab held the capture matrix, the outputs matrix and the stereo links, each
matrix capped at `62vh` with its own scrollbar inside a page that also scrolled. Reaching
a cell meant driving two vertical scrollbars against each other.

Fixed structurally rather than by tuning heights. The window is now the scroll container
— `html, body { height: 100% }`, body a flex column with `overflow: hidden` — and each
panel fills the space below the tabs with **exactly one** scrolling region inside it. The
`min-height: 0` on flex children is what actually makes that work; without it a flex item
is floored at its content height and the scrollbar escapes to the body.

Tabs regrouped:

- **Capture** and **Outputs** are now separate tabs, one matrix each.
- **Stereo links** moved to the CC Map tab. Not for tidiness: a link's entire significance
  is that it silently rewrites the CC map, so the toggle now sits directly above the map
  it rewrites and the consequence is visible in the same view.
- **Analogue** replaces Inputs, holding both DC controls — input blocking and output
  offsets. They are the same kind of thing, the analogue conditioning at each end, and
  neither is routing.

### Output DC offsets

The protocol and device layers already had `Action::SetDcOffset` and `encode::set_dc_offset`
from Phase 1; what was missing was the view and the UI. Range is ±3176, taken from the
official tool. The manual gives no units and the tool shows the bare number, so this does
too — inventing a voltage conversion that cannot be checked would be worse than a number.

**The constraint worth surfacing:** the manual notes in passing that the offset is
implemented *with the mixer hardware*, so an output with no mix routed to it ignores the
setting completely. The module gives no feedback about this, and a slider that silently
does nothing is exactly the failure this project exists to avoid. `view::dc_offsets`
therefore computes, per output, whether a mix is routed to it and which one, and the UI
marks the inert ones and says why. On the current rig that is Outputs 1–4 live and 5–8
inert, and the answer changes as soon as the routing does.

### DC on the meters

Peak and RMS both discard sign, so a channel resting at a constant voltage reads as
silence on both. `Meter` now tracks a smoothed mean as well, and the Meters tab has a DC
column that highlights anything past ±0.001.

This immediately paid for itself. On the attached module, inputs 3–8 (DC blocking **on**)
read −0.0000, while inputs 9–14 (blocking **off**) show real offsets between +0.0034 and
−0.0049. The two features corroborate each other, and the reported "idle channels have
random non-zero offsets" is now visible rather than inferred.

To trim an output against a measurement rather than by ear, patch it back to an input and
watch this column; the UI says so on both tabs.

### A screenshot artefact I mistook for a layout bug

Content looked clipped at the right edge on every capture, and I changed the window size
chasing it. Wrong: `innerWidth 1280, body.scrollWidth 1280, dpr 1.5` — nothing overflowed.
**The PowerShell screenshot harness was DPI-unaware**, so `GetWindowRect` returned
virtualised coordinates and the bitmap captured 1/1.5 of the real window. The fix is
`SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)` in the capture script, not anything
in the app. Making the harness DPI-aware also fixed synthetic clicks, which had been
landing at scaled-down coordinates and missing the tabs entirely.

Same lesson as the embedded-frontend confusion earlier: measure the thing before changing
it. The window default is now 1120×700 logical anyway, which fits a 150%-scaled 1080p
display without being clamped.

**104 tests, clippy and rustfmt clean, smoke test passing.**

## 2026-08-30 — Presets, factory defaults, and an installed application

### Two errors found before writing any of it

`0x26` **takes a parameter** — `1` for hosted defaults, `0` for standalone — and this
project's encoder sent it without one. The command table in the protocol doc also called
it "Reset flash to defaults", when the manual is explicit that it affects the *current*
configuration and leaves both flash slots alone. Reading the reference tool's `defaults()`
settled both: it sends `F0 00 21 27 19 26 <wh> F7`.

Worth noting what this means: the two default sets are the **module's own**, not the
tool's reconstruction. So the app sends `26H` and re-reads, rather than uploading
`presets::hosted()`, which is only our copy of them.

### H8 passed, which decided the design

Before building presets I ran the outstanding `09H` upload spike, because the answer chose
between two implementations: a whole-configuration upload, or rebuilding a configuration
from individual commands.

**All 24 chunks, sent back to back with no pacing at all, came back byte-identical.** Q4
and H8 both resolved. Uploading the configuration the module already had made the test
safe — a success changes nothing.

So `Action::LoadConfig(Box<Config>)` sends the upload and takes the previous configuration
as its inverse. Loading a preset is one undoable step, and undoing it is another upload
rather than a replay of a hundred small commands.

There is still no acknowledgement in the protocol, so nothing would report a partial
upload. The app therefore re-reads and compares after every load. "Measured reliable once"
is not "guaranteed", and a half-applied configuration would be worse than a refused one.

### Presets are the module's own dumps

A preset is the raw `08H` configuration dump in a plain `.syx` file, in
`%APPDATA%\es9-mixer\presets`. That means the archives the hardware probe writes before
every risky operation are loadable as presets with no conversion — which is the point,
since the dump taken before a change is exactly what you want when the change goes wrong.
One of those archives is now installed as a preset.

Files that will not parse are **listed with the reason and their Load button disabled**,
not hidden. A preset that silently vanished would be worse than one that explains itself;
verified with a deliberately corrupt file, which showed as "not an ES-9 message (header
mismatch)".

Preset names come from a text box and become file paths, so they are validated rather than
trusted: nothing empty, over 64 characters, leading-dot, or containing a separator or
control character.

`Session::record_undo` is new, for the factory reset: the module performs it, so there is
no action to invert, but it is still destructive and still ought to be reversible.

### Destructive actions arm before they fire

Reset and Load replace the entire configuration. Both now arm on the first click, change
appearance, and fire on the second, disarming after four seconds. A webview confirmation
dialog would have needed a plugin, and neither action should ever be one stray click away.

### Installed

`scripts/win-install.ps1` builds the release binary, copies it to
`%LOCALAPPDATA%\Programs\ES-9 Mixer`, and writes a Start Menu shortcut carrying the app's
icon. Per-user, so no elevation; re-running upgrades in place, stopping a running instance
first so the copy succeeds.

**Taskbar pinning is deliberately blocked by Windows** — applications have not been able
to pin themselves since Windows 10, and there is no supported API. The Start Menu shortcut
is the part that can be automated; pinning is one right-click by hand.

### What is verified and what is not

Verified on hardware: the installed release binary runs, connects, and shows the module;
the preset list parses a real archived dump; a corrupt file is reported rather than hidden.

**Not verified on hardware: actually loading a preset, and the factory reset.** Both
replace the live configuration, and doing that to a module sitting in someone's rig is not
a decision to make unasked. Both paths are covered by tests through the mock, and the
upload transport they rely on is H8-verified — but the buttons themselves have not been
pressed against the hardware.

**106 tests, clippy and rustfmt clean, smoke test passing.**
