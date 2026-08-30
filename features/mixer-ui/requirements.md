# ES-9 Mixer UI — Requirements

**Status: AGREED.** All open decisions resolved 2026-08-28.

Last updated: 2026-08-28

## 1. Goal

Replace the Expert Sleepers ES-9 Configuration Tool with an interface that makes the
device's routing and mixing legible at a glance and fast to adjust, for someone patching
in real time rather than configuring once.

### Primary use case (added 2026-08-28)

Ableton can *route* audio to and from the ES-9, but it cannot control the module's
internal gain and routing. The value of a dedicated tool is having the ES-9's own matrix
mixer under direct control while working modular — and, crucially, **carrying that
configuration into a DAW-free live setting** driven by a MIDI controller.

Two operating contexts, both first-class:

1. **Hosted, laptop attached** — the app is live: matrix mixing, routing, metering.
2. **Standalone, no computer** — the ES-9 runs from its flash config. The app's job was
   done beforehand: build the configuration, verify it, write it to the standalone slot.

The workflow that joins them is *preconfigure on the desktop, then perform without it.*
That makes standalone-config authoring a headline feature, not an afterthought.

### ⚠ Hardware constraint on standalone MIDI control

The ES-9's USB port is a **device** port, not a host port, so a USB MIDI controller cannot
be plugged into the module. **MIDI CC control with no computer present therefore requires
the Expert Sleepers DM4 DIN MIDI breakout** on the module's "GT2 – MIDI" header.

Without that breakout, the two contexts are:

- laptop attached → controller reaches the ES-9 through the computer, and CC control works
- fully standalone → **static preconfigured mix only**, no live control

**Resolved 2026-08-28: the breakout will be acquired.** Assume DIN MIDI is available. Both
MIDI paths are therefore supported and both matter:

- **hosted** — controller reaches the module through the computer over USB
- **standalone** — controller connects directly to the module's DIN input

Consequence: the resolved CC map (R26) governs live control in the standalone case with no
software able to intervene, which raises its importance rather than lowering it. The DIN
MIDI channel setting and MIDI Thru (standalone-only) are in scope.

## 2. Scope

**In scope**

- The mixer: 16 mixes x 8 channels, level and pan, stereo linking.
- Routing: capture and output assignment for all four blocks (`0x40+dsp` / `0x50+dsp`).
- Everything required to support those: config dump, mix dump, options (Mixer 2 enable),
  stereo links, gain smoothing, sample rate, connection management, flash save/restore.

**Out of scope for this feature** (protocol documented, UI deferred)

- Input EQ (`0x39`)
- Input DC blocking (`0x31`), output DC offsets (`0x36`)
- Codec reset/resync (`0x28` / `0x29`)
- Display messages (`0x02`), DSP usage readout (`0x2B`)

**Moved into scope 2026-08-28** — these turned out to be central to the live use case, not
peripheral:

- MIDI channel configuration (`0x35`) and MIDI Thru, since they gate CC control
- Flash slot management (`0x24` / `0x25` / `0x26`) as a first-class workflow
- Config file upload/download (`0x09`)

## 3. Platform constraints

**Revised 2026-08-28** after the metering research. Originally a browser app; changed
because metering is impossible in the browser (Chrome clamps audio input to 2 channels).

- **Tauri desktop application.** Web UI in a native shell, Rust backend.
- **Target OS: Windows.** The ES-9 lives on the Windows host; WSL2 is the editing
  environment only.
- **MIDI:** `midir` (Rust) for SysEx in and out.
- **Audio:** `cpal` with the ASIO backend for 16-channel capture. Requires the Steinberg
  ASIO SDK at build time (`CPAL_ASIO_DIR`) plus LLVM/Clang. The SDK is not redistributable
  but building against it is permitted.
- **Firmware target: 1.3.x only.** Firmware 1.2 uses a different config dump format; it is
  detected (by command byte, not the version field) and the user is told to update.
- State persists to disk (labels, layout, snapshots).

### Why Tauri rather than Electron

`cpal` has a maintained ASIO backend, and the ES-9 needs ASIO on Windows for anything past
stereo. The Electron equivalent is `naudiodon`/PortAudio bindings, which need a native
rebuild per Electron version and are less actively maintained. MIDI (`midir`) and audio
(`cpal`) then live in the same language as the protocol codec.

### Build environment caveat

The repo is in WSL2 but the artefact is a Windows binary. Cross-compiling Tauri from Linux
to Windows is awkward, and the ASIO SDK is Windows-only. Expect to run the Rust/Node
toolchain **on Windows**, with WSL2 used for editing. Settle this before Phase 1.

## 3a. Metering

- **M1.** Meter the ES-9's physical inputs and its mix outputs (D5).
- **M2.** Metering is derived host-side from the USB capture stream. The device reports no
  audio levels — only DSP load.
- **M3.** Metering must degrade gracefully: if the audio device cannot be opened, the app
  remains fully functional as a mixer and router, and says why meters are unavailable.
- **M4.** Where metering a point requires reserving a USB capture channel, the app must
  show that it has done so and let the user decline.
- **M5.** Gated on spike S0 — whether a 16-channel capture stream can be opened at all,
  and whether it can be opened alongside a DAW. **Risk downgraded 2026-08-28:** the
  primary live context is DAW-free, so ASIO exclusivity does not block the main use case.
  It remains a known limitation for hosted DAW sessions.

## 4. Hardware availability

The ES-9 is **not connected** during initial development. Consequences:

- The protocol layer must be exercisable against a **mock device** that replays and
  responds to dumps, so logic is testable offline.
- Every claim in `docs/es9-sysex-protocol.md` marked as an open question is treated as an
  assumption, isolated behind a single code path, and listed in the hardware
  verification checklist (`plan.md` §6).
- The app ships a **protocol monitor** view (raw TX/RX hex with decode) so that when the
  device arrives, verification is a matter of reading the log rather than adding
  instrumentation.

## 5. Problems with the reference tool

These are the specific things "more intuitive" is measured against.

| # | Problem | Evidence in v1.2 |
|---|---|---|
| P1 | Everything on one flat page at once — 16 raw mix tables, 16 virtual mix tables, 64 routing dropdowns, 64 EQ strips | single `<body>`, no sections or navigation |
| P2 | Two mixers shown side by side with no stated relationship | "Raw mix - the actual mix matrix running on the hardware" is the only explanation given |
| P3 | Signal flow is invisible — routing is 64 unlabelled dropdowns in a grid | `writeCaptureSelect` / `writeOutputSelect` |
| P4 | Sources are opaque identifiers with no user labels | options read "Bus 5", "MIX 3", "Input 11" |
| P5 | Controls are 30px and 15px wide sliders with no numeric entry | `td.mix {width:30px}`, `td.rawmix {width:15px}` |
| P6 | No undo, no snapshots beyond the device's two flash slots | only `saveToFlash(0/1)` |
| P7 | No state model — DOM is the source of truth, rebuilt via `document.write` | `rebuildMixer()` regenerates `innerHTML` |
| P8 | Silent failure on firmware mismatch | `alert()` then `return` in `parseConfigDump` |

## 6. Functional requirements

### 6.1 Connection

- R1. Enumerate MIDI ports; let the user pick input and output, or auto-select a port
  whose name matches the ES-9.
- R2. On connect, request version (`0x22`), sample rate (`0x2C`), and config (`0x23`).
- R3. Show firmware version and sample rate persistently.
- R4. Handle a config-dump version other than `3` by warning and entering a read-only
  mode, not by aborting.
- R5. Detect disconnect via `onstatechange` and surface it; reconnect without losing
  local labels or snapshots.

### 6.2 Mixer

- R6. Present each mix as a channel strip set: 8 source channels, each with level and,
  when stereo-linked, pan.
- R7. Show level in dB, with direct numeric entry as well as a drag control.
- R8. Double-click resets a level to 0 dB and a pan to centre (matching v1.2's `v = 103`
  default).
- R9. Show which mixes are available given the Mixer 2 option, and let the option be
  toggled.
- R10. Stereo link state is editable and immediately restructures the strips.
- R11. Per-mix gain smoothing (`0x3A`) is exposed as a per-mix toggle.
- R12. The virtual mixer (level + pan) is the primary interface. The raw 21-bit gain
  matrix is reachable through an explicit "advanced" disclosure, not shown by default.

### 6.3 Routing

- R13. Show capture and output assignment for all four blocks.
- R14. Present the ES-9's permuted input ordering in natural order (Input 1..14), hiding
  `inputCaptureLookup` from the user.
- R15. Routing is presented as a patchbay matrix: sources as rows, destinations as
  columns, connections toggled at the intersection. Grouped by source family (Inputs,
  Buses, USB, MIX, S/PDIF) with collapsible groups.

### 6.4 Labels and state

- R16. Every source and destination can be given a user label, stored locally, shown
  everywhere that source appears.
- R17. Local snapshots: capture the full device state, name it, restore it. Independent
  of the device's two flash slots.
- R18. Device flash save/restore (hosted and standalone) remain available and clearly
  distinguished from local snapshots.
- R19. Undo/redo across all device-affecting edits.

### 6.5 Modes, flash slots, and the standalone workflow

- R22. The app always displays which mode the module is in (hosted/standalone) and which
  flash slot was last loaded.
- R23. **Editing the standalone configuration is a guarded, explicit workflow.**
  Connecting over USB forces hosted mode, so editing the standalone config requires
  restoring slot 0 first. The app must make this a single deliberate action — "edit
  standalone config" — that performs the restore, tracks that it is in standalone-editing
  mode, and saves back to slot 0. It must never let the user believe they are editing the
  standalone config when they are editing hosted state.
- R24. Unsaved-change tracking against the loaded slot, since `0x26` reset and live edits
  do not touch flash.
- R25. Config file download and upload (`0x09`), with read-back verification because the
  upload has no acknowledgement.

### 6.6 MIDI CC map

- R26. **Resolved CC map view** (D6): for the current link and Mixer 2 configuration, show
  exactly which CC number drives which control — e.g. "CC 8 → pan, mix 1/2, channel 1".
- R27. The map must be derived from the documented rules, including the stereo cases: use
  the lower-numbered CC of a stereo pair, and pan takes the CC the right-channel mix would
  have used.
- R28. **Warn when a link change rewrites the CC map.** Toggling a stereo link silently
  changes what existing controller mappings do. The app must show what changed.
- R29. MIDI channel assignment for USB and DIN ports, including Off.
- R30. The CC map is exportable, so a controller can be mapped against it away from the
  app.

### 6.7 Protocol monitor

- R20. Live TX/RX log with hex bytes and a decoded interpretation per message. Because
  `0x34`'s index byte *is* the CC number, this doubles as the manual's own recommended
  method for identifying a control's CC — promote that to a labelled feature rather than
  leaving it as a debugging trick.
- R21. Ability to send an arbitrary SysEx message by hand (the feature v1.2's broken
  `send()` intended to provide).

## 7. Non-functional requirements

- N1. The protocol codec is a standalone Rust module with no Tauri, MIDI, or UI
  dependency, unit-testable against captured byte arrays.
- N2. Application state lives in a plain data model; the DOM renders from it. No
  `document.write`, no reading values back out of DOM elements.
- N3. Protocol encode/decode is a separate layer with no DOM dependency, unit-testable
  against captured byte arrays.
- N4. Every outbound message is emitted through one function so the monitor and undo
  stack see all traffic.
- N5. Continuous controls throttle SysEx output during a drag (v1.2 sends on every
  `oninput`).

## 8. Resolved decisions

Settled 2026-08-28.

**D1 — Raw vs virtual mixer: virtual primary, raw advanced.**
Level + pan strips are the default and only visible mixer. The raw 21-bit matrix is
available behind an explicit disclosure for cases needing finer resolution than the
7-bit scale allows. Consequence: the UI must state which layer the user is editing, and
§7 open question 7.6 (does a raw write get clobbered by the stored virtual value?) is
promoted to a **blocking** hardware check before the advanced panel is trusted.

**D2 — Routing: patchbay matrix.**
Sources as rows, destinations as columns, cells toggled. Chosen over a flow diagram for
edit speed and over the v1.2 grid for showing all connections simultaneously. Note the
underlying model is *not* a free matrix: each of the 32 capture channels takes exactly
one source, and each of the 32 output channels takes exactly one destination. So each
column has at most one active cell, and clicking a cell moves the assignment rather than
adding one. This constraint must be visible in the interaction, not just enforced.

**D3 — Density: console layout.**
Large faders, one mix in focus at a time, with a selector to switch mixes. Prioritises
real-time adjustment over simultaneous visibility.

**D4 — Metering: none.** ~~superseded~~

**D4 (revised 2026-08-28) — Metering: yes, host-side, from the USB capture stream.**
The protocol genuinely has none, so it is derived by analysing the audio the ES-9 sends
the host. This forced the platform change from browser to Tauri: Chrome clamps
`getUserMedia` audio input to 2 channels, so a browser app could never meter more than one
stereo pair.

**D5 — Meter scope: inputs and mix outputs.**
Capture channels are reserved as needed to guarantee those points are visible, with the
reservation made explicit in the UI (M4).

**D6 — MIDI control: resolved CC map view, not a translation layer.**
The hardware's CC numbers are fixed, so the app shows the truth rather than inventing
mappings the module will not honour. A translation layer (remapping an arbitrary
controller through the computer) is deferred — it only works laptop-attached, and would
not survive the standalone case.

**D7 — Both operating contexts are first-class.**
Sometimes laptop-attached without a DAW, sometimes fully standalone. Consequence: the
standalone-config authoring workflow (R23) carries the same weight as the live mixer, and
metering is a hosted-only enhancement rather than a core promise.

## 9. Acceptance criteria

- A1. With no device connected, the app loads, shows a clear "no ES-9 found" state, and
  the mock device can drive every view.
- A2. Connecting a device populates routing, mixer, links and options from a single
  config dump with no further user action.
- A3. Moving a fader produces exactly one `0x34` message per settled value, not one per
  pointer event.
- A4. A round trip — change state, request config, reparse — reproduces the same UI
  state, for every field in scope.
- A5. Undo restores both the UI and the device to the prior state.
- A6. Every byte sent or received appears in the protocol monitor, decoded.
- A7. A firmware reporting a config version other than 3 yields a warning and read-only
  mode, never a blank or broken page.
