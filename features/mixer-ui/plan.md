# ES-9 Mixer UI — Implementation Plan

Revised 2026-08-28 after the metering research. Supersedes the browser-only plan.

Derived from `requirements.md` and `../../docs/es9-sysex-protocol.md`.

## 0. Phase 0 — Spikes (blocking, requires hardware)

Two unknowns can invalidate the design. Neither is expensive to test, and both must be
answered **before** Phase 3 or 4 begin.

### S0 — Can we open a 16-channel capture stream? — ✅ RESOLVED 2026-08-30

The ES-9 needs the Expert Sleepers **ASIO** driver on Windows for more than stereo, and
ASIO drivers are conventionally single-client.

**Downgraded from blocking 2026-08-28.** The primary live context is *DAW-free*, so there
is nothing to contend with for the use case that matters most. ASIO exclusivity is now a
known limitation of hosted DAW sessions, not a threat to the design. What S0 must still
establish is whether a 16-channel stream can be opened **at all**.

*Test:* start the DAW on the ES-9 ASIO driver, then run a small `cpal` probe that
enumerates devices and attempts a 16-channel capture stream.

**Answered on hardware. Metering works, via ASIO — but not for the reason expected.**

WASAPI *does* expose all 16 capture channels, which looked like the "drop the ASIO SDK"
row. It is not, because WASAPI shared mode can only run at the Windows endpoint's
configured format. The endpoint was set to 32 kHz, an explicit 48 kHz request was refused
outright, and **opening the meter stream re-clocked the module from 48 kHz to 32 kHz**,
where it stayed until ASIO restored it. A tool that silently changes the rig's clock to
draw its meters is not acceptable, so WASAPI is rejected on clock integrity — not on
latency, which is irrelevant here since nothing is in the signal path.

ASIO, via the Steinberg SDK's GPLv3 option (`github.com/audiosdk/asio`):

- advertises 16 channels at **every** rate the module supports, not just one;
- opened 16 channels @ 48 kHz, ≈2.7 ms blocks, live meters on 14 channels;
- restored the module to 48 kHz rather than imposing a rate.

| Result | Response |
|---|---|
| 16-ch capture works, driver is multi-client | **Untested** — no DAW was running during the spike |
| 16-ch capture works, ASIO is exclusive | Meters work DAW-free; disabled with an explanation when a DAW holds the driver |
| ~~WASAPI shared mode exposes all 16 channels~~ | ~~Drop the ASIO dependency~~ — **rejected: shared mode re-clocks the module** |
| No path to more than stereo | Ruled out |

**Consequence:** `crates/es9-audio` links the ASIO SDK under GPLv3, so that crate and
anything linking it are GPL-3.0-only. The four pure crates stay MIT.

### S1 — Does the raw mix matrix hold its value? — ✅ RESOLVED 2026-08-30

Smoothing is documented as interpolating macro → raw, implying the macro layer
continuously drives the raw matrix. If so, a direct `0x60+mix` write is transient and the
"advanced raw panel" (D1) is not meaningful.

*Test:* write a raw gain, wait, re-read via `0x2A`. Then move a macro fader on the same
cell and re-read.

**Answered: macro drives raw.** The raw write sticks — `0x4000` written, `0x4000` read
back — but a macro write to the same cell then replaces it. So a raw value survives only
until anything moves the corresponding macro control, including a CC from a controller,
which in the live rig is continuous.

**Step 15 is dropped.** Offering a control whose value silently will not survive normal
use is worse than not offering it. The mixer is macro-only, and D1's advanced disclosure
goes with it. R-2 resolved as feared.

## 0a. Phase 7 — Desktop shell — ✅ DONE 2026-08-30

`crates/es9-app`, a Tauri shell. Runs against the real module: auto-connects over MIDI,
loads the configuration, and meters sixteen channels at 48 kHz over ASIO.

- **Not a workspace member.** Tauri pulls GTK and dbus on Linux, which WSL2 lacks and the
  app could never use. It is its own workspace; build it with `scripts/win-build.ps1 -App`.
- **No Tauri CLI and no Node.** A Tauri app builds with plain `cargo build` given a
  `build.rs` and `tauri.conf.json`, and the frontend is static.
- **One frontend, two backends.** `web/backend.js` chooses Tauri's `invoke` or the WASM
  mock at load time, so the browser prototype and the shell share `web/` unforked.
- **The module is found by asking.** Windows publishes it under a generic jack name, so
  the shell version-requests every port pair and keeps the one that answers.
- **Requests go one at a time.** The protocol has no flow control and a dump is 2313
  bytes; firing the startup requests together loses the dump.

⚠️ **`frontendDist` is baked into the binary at compile time.** Editing `web/` requires
rebuilding the shell, or you are looking at the previous UI.

Outstanding: no port picker in the UI (autoconnect only).

## 1. Shape of the build

```
src-tauri/
  src/
    protocol/    frame.rs, encode.rs, decode.rs, tables.rs, scales.rs   — pure, no I/O
    midi.rs      midir wrapper; all traffic passes through one point
    audio.rs     cpal capture + peak/RMS meter computation
    state.rs     device model, action application
    main.rs      Tauri commands and events
  tests/         round-trip tests against fixtures
src/             web UI (TypeScript, no framework required)
```

The protocol codec is pure Rust with no Tauri, MIDI, or UI dependency (N1), so it can be
tested without hardware. The UI receives state as typed events and sends actions as
commands; it never constructs SysEx itself.

## 2. Phase 1 — Protocol layer (no hardware needed) — ✅ DONE 2026-08-28

Built to the **firmware 1.3 word-array format**, per the manual's SysEx appendix.

1. `frame.rs` — header match, 21-bit and 14-bit split/join, NUL-terminated string
   handling for `32H`.
2. `tables.rs` — capture-source and output-destination enums, the `inputCaptureLookup`
   permutation and its inverse, link bitmap layout.
3. `scales.rs` — raw gain ↔ dB (16-bit), macro level ↔ dB (7-bit piecewise), EQ freq/Q/
   gain, sample rate. Property-test the inverses.
4. `encode.rs` — one builder per outbound command, including `09H` chunked upload.
5. `decode.rs` — `08H` config dump (768 words), `11H` mix dump, `12H`, `14H`, `32H`,
   `60H+mix` echo. Reject a `10H` dump with a clear "firmware 1.2, please update" error
   rather than misparsing it.
6. Round-trip test: synthetic config → encode → decode → deep equality.

**Exit:** every command in the protocol doc has an encoder and every inbound message a
decoder; round-trip passes; a 1.2-format dump is rejected rather than silently misread.

**Met.** `crates/es9-protocol`, 29 tests, clippy and rustfmt clean. Steps 19e (resolve)
and 19g (diff) of Phase 4c landed with it, since the CC map is pure logic over the same
data.

## 3. Phase 2 — Transport and state — ✅ DONE 2026-08-28

7. `midi.rs` — port enumeration, auto-select by name, SysEx in/out, and a single choke
   point through which all traffic passes so the monitor and undo stack see everything
   (N4). Outbound messages tagged so echoes can be distinguished (Q6).
8. `state.rs` — device model plus an action dispatcher that applies to state and emits
   bytes together.
9. Undo/redo via inverse actions (R19).
10. **Mock device** implementing the same protocol, so the whole app runs with no
    hardware (A1).
11. Send throttling so a fader drag coalesces to one message per settled value (N5, A3).

**Exit:** full session round-trips against the mock; undo works.

**Met.** `es9-device` (pure: state, actions, undo, mock, coalescer) and `es9-midi`
(`midir`, SysEx reassembly, monitor). 57 tests across the workspace, clippy and rustfmt
clean. The protocol monitor from Phase 6 step 27 landed here too, since the transport is
where all traffic already passes.

## 4. Phase 3 — Console mixer (D3) — ✅ prototype done 2026-08-28

12. Mix selector — 16 mixes, availability driven by the Mixer 2 option, each labelled from
    block 2/3 output routing.
13. Channel strip — large fader, dB readout with numeric entry, pan when stereo-linked,
    double-click reset.
14. Stereo link editing; per-mix smoothing toggle.
15. ~~Advanced raw-matrix disclosure~~ — **DROPPED 2026-08-30.** S1 showed the macro
    layer overwrites raw values, so an editable raw control would not hold its value.
    The prototype's raw readout stays, because the numbers are the gains the hardware is
    really running, but it is explicitly read-only and says what overwrites it.

## 5. Phase 4 — Patchbay routing (D2) — ✅ prototype done 2026-08-28

16. Matrix with sources grouped by family, destinations by block.
17. Enforce and *show* one-source-per-channel: clicking moves an assignment rather than
    adding one.
18. Natural input ordering 1–14, hiding the permutation.
19. Stereo-link-aware assignment.

## 6. Phase 5 — Metering (D4, D5) — ✅ ungated 2026-08-30 (S0 passed)

`crates/es9-audio` exists: `meter.rs` (peak/RMS ballistics, peak hold, clip latch, 9
tests) and an `audioprobe` binary that is the S0 spike. Steps 20–23 below are what remains.

20. ✅ **DONE** — `es9-audio::capture` opens the stream (ASIO preferred, since WASAPI
    re-clocks the module) and `es9-audio::meter` computes peak and RMS with ballistics
    and peak hold.
21. ✅ **DONE** — meter channel *i* is USB capture channel *i*, so each bar is labelled
    from the routing table and follows it when the routing changes.
22. Reserve capture channels where needed to meter inputs and mix outputs, shown
    explicitly and declinable (M4).
23. ✅ **DONE** — audio opens independently of MIDI, and a failure is carried to the UI
    as `audioError` rather than leaving an empty panel.

## 6a. Phase 4b — Modes, flash slots, and the standalone workflow

The join between the two operating contexts, and the highest-value part of the app for the
preconfigure-then-perform workflow.

19a. Mode and slot indicator — always show hosted vs standalone and which slot was last
     loaded.
19b. **Guarded "edit standalone config" action.** Performs `0x25` restore from slot 0,
     enters a clearly-marked standalone-editing mode, and saves back to slot 0. Prevents
     the failure the reference tool permits: believing you are editing the standalone
     config while actually editing hosted state.
19c. Dirty-state tracking against the loaded slot.
19d. Config file download and `0x09` upload, with read-back verification (Q4).

## 6d. Phase 4e — Output DC offsets — ✅ DONE 2026-08-30

Second scope addition, for the same reason as DC blocking: needed on the rig.

19n. `view::dc_offsets` — per-output trim, plus **whether the trim can do anything**. The
     module implements offsets with the mixer hardware, so an output with no mix routed
     to it ignores the setting silently; the view computes this and the UI marks it.
19o. DC column on the meters. Peak and RMS discard sign, so a constant offset is invisible
     to both; the mean is the only reading that shows it, and it is what an output trim is
     adjusted against once the output is patched back to an input.

Range is ±3176 with no documented units, so the bare number is shown rather than an
invented voltage.

## 6c. Phase 4d — Input DC blocking — ✅ DONE 2026-08-30

Scope addition: the original plan excluded DC blocking, but it is needed for the rig.

19j. `es9-protocol::dcblock` — the seven input pairs, bit/input mapping, labels. The
     pairing is non-obvious (**inputs 3 and 8 share a filter**) because a filter covers
     two adjacent ADC channels and input numbers are permuted on the wire. A test derives
     the pairing from `INPUT_CAPTURE_LOOKUP` so it cannot be "tidied" into sequential
     pairs. See `docs/es9-sysex-protocol.md` §10.
19k. `Action::SetDcBlock { bit, on }` — per-pair, so undo restores one filter.
19l. `view::dc_filters` with a `surprising` flag for the non-adjacent pair.
19m. Inputs tab. Cards say what the setting *means* — "filtered — for audio" versus
     "unfiltered — passes CV" — since on/off alone does not tell you which breaks a CV.

**Outstanding:** H11 — which physical inputs a bit governs is inferred from the
permutation, not measured. Confirming it needs a DC offset on one input of a pair.

## 6b. Phase 4c — MIDI CC map (D6) — resolver ✅ DONE, UI outstanding

19e. Derive the resolved CC map from the current link and Mixer 2 configuration, including
     both stereo rules (lower CC of a pair; pan takes the right-channel mix's CC).
19f. Present it as a readable table and inline on each control.
19g. **Diff the map across a link change** and show what a toggle just rewrote.
19h. MIDI channel assignment for USB and DIN, including Off.
19i. Export the map for offline controller mapping.

Note this phase needs **no audio and no metering**, and delivers value in the fully
standalone context. If S0 goes badly, this is the part that still matters.

## 6e. Phase 4f — Presets and factory defaults — ✅ DONE 2026-08-30

Third scope addition, and the one that makes the flash workflow safe to experiment with.

19p. **Factory defaults.** `26H <wh>` — the module's own reset, `1` hosted / `0`
     standalone. It affects the current configuration only, never flash, and the UI says
     so. Not undoable by inverse, so the previous configuration is captured first and
     recorded via `Session::record_undo`.
19q. **User presets.** A preset is the module's own `08H` configuration dump stored as a
     plain `.syx`, so the archives the hardware probe writes are loadable without
     conversion — the dump taken before a risky change is exactly the thing wanted when it
     goes wrong. Stored in `%APPDATA%\es9-mixer\presets`.
19r. **`Action::LoadConfig`** applies a whole configuration via the `09H` upload, with the
     previous configuration as its inverse, so loading a preset is one undoable step. The
     upload is verified by reading back: H8 measured it reliable, but nothing in the
     protocol would report a failure, so it is checked rather than assumed.
19s. Unloadable files are listed with the reason rather than hidden.

Destructive actions arm on the first click and fire on the second, so a preset load or a
factory reset is never one stray click away.

## 7. Phase 6 — Labels, snapshots, monitor

24. User labels for every source and destination, persisted, shown everywhere.
25. ✅ **DONE** — local presets, distinct from the device's two flash slots. See §6e.
26. Config file save/load via `09H` chunked upload — with read-back verification, since
    there is no acknowledgement (Q4).
27. Protocol monitor: TX/RX hex plus decode, and a hand-send field.

## 8. Hardware verification checklist

Reduced from the previous version — the manual settled most of it.

Run with `crates/es9-midi/src/bin/probe.rs` — read-only by default, and it archives a
full dump before anything else. The write-side checks sit behind `--write-tests`.

| # | Check | Blocking? | Status |
|---|---|---|---|
| S0 | Multichannel capture | ~~Gates metering~~ | ✅ **16 ch @ 48 kHz via ASIO.** WASAPI rejected — it re-clocks the module |
| S1 | Raw mix values persist against the macro layer (Q5) | ~~Gates step 15~~ | ✅ **Macro drives raw.** Step 15 dropped |
| H1 | Config dump is 768 words, command `08H` | Yes | ✅ **PASS** — 2313 bytes, cmd `0x08`, 768 words, decoded first time |
| H2 | Macro-mix word layout in the dump (Q1) | Yes | ✅ **`[0..128]` levels by CC, `[128..256]` pans by CC.** Confirmed by a write and read-back; pans still written verbatim |
| H3 | Pan row for a stereo pair (Q3) | Yes | ✅ **Written to CC `(m+1)*8+ch`, stored in the level cell's aux as `written - 1`.** Retracts the "reference tool is buggy" note |
| H4 | Pan centre bias, 63 or 64 (Q2) | Yes | ✅ **Both.** Written centre 64, stored centre 63 — the module subtracts one. The first answer here was wrong |
| H5 | Does the device echo the host's own writes (Q6) | Yes — feedback guard | ✅ **Yes, and more:** a macro write draws a full `0x11` mix dump. Feedback guard required; coalescing is a bandwidth need |
| H6 | Physical input permutation matches `inputCaptureLookup` | Yes | Table decodes plausibly; not confirmed against the physical patch |
| H7 | Output permutation, incl. the `Output 7 = 11` / `Output 8 = 10` swap | Yes | As H6 |
| H8 | `09H` upload reliability without flow control (Q4) | No | ✅ **PASS.** 24 chunks, no pacing, read back byte-identical |
| H9 | Reserved dump words 719–767 | No | ✅ All zero |
| H10 | DC-blocking bitmap is seven bits, one per input pair | Yes | ✅ **PASS.** Single-bit walk round-tripped for all 7; `0xFF` read back `0x7F` |
| H11 | Which physical inputs each DC bit governs | Yes | **Inferred, not measured.** Corroborated by the input permutation; needs a CV on one input of a pair |

Firmware reports **1.3.1**.

### H0 — the module cannot be found by name (new, blocking)

`midir`'s default WinMM backend does not see the ES-9 at all: the module is published
through **Windows MIDI Services** (`SWD\MIDISRV\MIDIU_KSA_...`), and switching `midir` to
its `winrt` backend does not change the enumeration either. The port *is* present, named
**`2 - MIDI In` / `2 - MIDI Out`** — a generic jack name containing no "ES-9".

So `find_es9`'s name matching cannot work on Windows. The app needs an explicit port
picker, and should confirm the choice with a version handshake rather than trusting a
name. Until then every tool takes `--in`/`--out` indices.

**Before any config upload testing: save both flash slots and archive a full dump.**

## 9. Risks

- **R-0. ASIO exclusivity.** ~~Blocking.~~ Downgraded, and **still untested** — no DAW was
  running during the S0 spike, so whether the driver is multi-client remains unknown. The
  primary live context is DAW-free, so this costs meters only in hosted DAW sessions.
- **R-8. WASAPI re-clocks the module.** *(new, 2026-08-30)* Opening a WASAPI shared-mode
  capture stream drags the ES-9 to whatever rate the Windows endpoint is configured for,
  and shared mode refuses any other rate. This is why metering is ASIO-only. If a WASAPI
  fallback is ever added for machines without the driver, it must read the module's rate
  over SysEx first and refuse to open on a mismatch rather than silently re-clocking.
- **~~R-6. Standalone MIDI control needs hardware we may not have.~~ RESOLVED** — the DIN
  breakout will be acquired. Assume DIN MIDI exists. Both MIDI paths are in scope, and the
  standalone path has no software in the loop, so the resolved CC map is the only thing
  standing between the user and a silently wrong controller mapping.
- **R-7. Link toggles silently rewrite the CC map.** A stereo link changes what half the CC
  numbers mean, so a controller mapping is valid only for one link configuration. R28
  mitigates by surfacing the diff; the underlying hardware behaviour cannot be changed.
- **~~R-1. Build environment split.~~ RESOLVED 2026-08-30** — Windows `cargo` builds
  directly from the `\\wsl$` path, so the working copy stays in WSL2 and only artefacts are
  Windows-side. No cross-compilation, no second checkout. See `scripts/win-build.ps1`.
  Note WSL2 has no sound subsystem and no USB passthrough, so *nothing* touching hardware
  can be tested from the Linux side.
- **~~R-2. Macro drives raw.~~ CONFIRMED 2026-08-30** — D1's advanced panel is dropped and
  the mixer is macro-only. Found early, as hoped, so the cost was one spike.
- **R-3. Firmware format drift.** Already bitten once: 1.2 → 1.3 changed the dump format
  while leaving the version field at 3. Discriminate on the command byte and fail loudly.
- **R-4. Pan semantics.** Three interacting unknowns (Q1, Q2, Q3) and a reference
  implementation whose read and write disagree. Do not trust the tool here; derive
  behaviour from hardware.
- **~~R-5. ASIO SDK friction.~~ RESOLVED 2026-08-30** — LLVM was already installed and
  `asio-sys` built without incident. The cost is licensing, not build friction: taking the
  SDK's GPLv3 option makes `es9-audio` GPL-3.0-only.
