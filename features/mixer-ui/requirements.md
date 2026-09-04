# ES-9 Mixer — Requirements

The contract this project is measured against. Settled; changes to it are decisions, not
discoveries.

## 1. Goal

Replace the Expert Sleepers ES-9 Configuration Tool with an interface that makes the
device's routing and mixing legible at a glance and fast to adjust, for someone patching
in real time rather than configuring once.

### Primary use case

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

### MIDI paths

The ES-9's USB port is a **device** port, not a host port, so a USB MIDI controller cannot
plug into the module. CC control with no computer present therefore requires the Expert
Sleepers **DM4 DIN MIDI breakout** on the module's "GT2 – MIDI" header. It is present, so
both paths are supported and both matter:

- **hosted** — the controller reaches the module through the computer over USB
- **standalone** — the controller connects directly to the module's DIN input

In the standalone case no software can intervene, so the resolved CC map (R26) is the only
thing standing between the user and a silently wrong controller mapping.

## 2. Scope

**In scope**

- The mixer: 16 mixes × 8 channels, level and pan, stereo linking.
- Routing: capture and output assignment for all four blocks (`0x40+dsp` / `0x50+dsp`).
- Config dump, mix dump, options (Mixer 2 enable), stereo links, gain smoothing, sample
  rate, connection management.
- MIDI channel configuration (`0x35`) and MIDI Thru — they gate CC control.
- Flash slot management (`0x24` / `0x25` / `0x26`) as a first-class workflow.
- Config file upload and download (`0x09`).
- Input DC blocking (`0x31`) and output DC offsets (`0x36`) — needed on the rig.

**Out of scope** (protocol documented, UI deferred)

- Input EQ (`0x39`)
- Codec reset/resync (`0x28` / `0x29`)
- Display messages (`0x02`), DSP usage readout (`0x2B`)

## 3. Platform constraints

- **Tauri desktop application.** Web UI in a native shell, Rust backend.
- **Target OS: Windows.** The ES-9 lives on the Windows host; WSL2 is the editing
  environment only, and cannot reach the hardware at all.
- **MIDI:** `midir` for SysEx in and out.
- **Audio:** `cpal` with the ASIO backend for 16-channel capture. Requires the Steinberg
  ASIO SDK at build time (`CPAL_ASIO_DIR`) plus LLVM/Clang. The SDK is not redistributable
  but building against it is permitted; taking its GPLv3 option makes `es9-audio` and
  anything linking it GPL-3.0.
- **Firmware target: 1.3.x only.** Firmware 1.2 uses a different config dump format; it is
  detected — by command byte, not the version field — and the user is told to update.
- State persists to disk (labels, layout, snapshots).

### Why Tauri rather than Electron

`cpal` has a maintained ASIO backend, and the ES-9 needs ASIO on Windows for anything past
stereo. The Electron equivalent is `naudiodon`/PortAudio bindings, which need a native
rebuild per Electron version and are less actively maintained. MIDI, audio and the
protocol codec then all live in the same language.

## 3a. Metering

- **M1.** Meter the ES-9's physical inputs and its mix outputs (D5).
- **M2.** Metering is derived host-side from the USB capture stream. The device reports no
  audio levels — only DSP load.
- **M3.** Metering must degrade gracefully: if the audio device cannot be opened, the app
  remains fully functional as a mixer and router, and says why meters are unavailable.
- **M4.** Where metering a point requires reserving a USB capture channel, the app must
  show that it has done so and let the user decline.
- **M5.** Metering must never change the rig's clock. Any capture path that would re-rate
  the module is disqualified regardless of what else it offers.

## 4. Offline development

The app must run fully against a **mock device** with no hardware attached, because the
hardware is on another machine and most development happens without it.

- The protocol layer is exercisable against a mock that replays and responds to dumps.
- Every claim in `docs/es9-sysex-protocol.md` still marked as an open question is treated
  as an assumption, isolated behind a single code path, and listed in `plan.md` §2.
- The app ships a **protocol monitor** view — raw TX/RX hex with decode — so verifying a
  behaviour against hardware is a matter of reading the log rather than adding
  instrumentation.

## 5. Problems with the reference tool

The specific things "more intuitive" is measured against.

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

- R1. Enumerate MIDI ports and let the user pick input and output. The module cannot be
  identified by name on Windows, so a choice is confirmed by version handshake.
- R2. On connect, request version (`0x22`), sample rate (`0x2C`), and config (`0x23`) —
  one at a time, since the protocol has no flow control.
- R3. Show firmware version and sample rate persistently.
- R4. Handle a config-dump version other than `3` by warning and entering a read-only
  mode, not by aborting.
- R5. Detect disconnect and surface it; reconnect without losing local labels or snapshots.

### 6.2 Mixer

- R6. Present each mix as a channel strip set: 8 source channels, each with level and,
  when stereo-linked, pan.
- R7. Show level in dB, with direct numeric entry as well as a drag control.
- R8. Double-click resets a level to 0 dB and a pan to centre.
- R9. Show which mixes are available given the Mixer 2 option, and let the option be
  toggled.
- R10. Stereo link state is editable and immediately restructures the strips.
- R11. Per-mix gain smoothing (`0x3A`) is exposed as a per-mix toggle.
- R12. The macro mixer (level + pan) is the only editable mixer. The raw gain matrix is
  shown **read-only**, because the macro layer overwrites it, and says what overwrites it.

### 6.3 Routing

- R13. Show capture and output assignment for all four blocks.
- R14. Present the ES-9's permuted input ordering in natural order (Input 1..14), hiding
  `INPUT_CAPTURE_LOOKUP` from the user.
- R15. Routing is presented as a **patchbay**: two rows of jacks with cables strung
  between them, not a grid of cells. The model is not a free matrix — each capture channel
  takes exactly one source and each block output exactly one destination, while the other
  side of every connection fans out or sums. A grid states none of that and needs the rule
  written underneath it in prose; a jack holds one plug, so repatching visibly **moves**
  it. The constraint must be carried by the interaction, not merely enforced by it.
- R15a. A jack carrying more than one cable is marked. On the source side that is one
  signal feeding several channels; on the destination side it is the module **summing**
  them, which is how both mixer banks reach the main outs and is easy to create by
  accident. Neither is visible in a grid without counting cells.
- R15b. Both bays live on one tab, in signal order — what the module captures, then where
  it sends.

### 6.4 Labels and state

- R16. Every source and destination can be given a user label, stored locally, shown
  everywhere that source appears.
- R17. Local snapshots: capture the full device state, name it, restore it. Independent of
  the device's two flash slots.
- R18. Device flash save/restore (hosted and standalone) remain available and clearly
  distinguished from local snapshots.
- R19. Undo/redo across all device-affecting edits.

### 6.5 Modes, flash slots, and the standalone workflow

- R22. The app always displays which mode the module is in (hosted/standalone) and which
  flash slot was last loaded.
- R23. **Editing the standalone configuration is a guarded, explicit workflow.**
  Connecting over USB forces hosted mode, so editing the standalone config requires
  restoring slot 0 first. This is a single deliberate action that performs the restore,
  tracks that it is in standalone-editing mode, and saves back to slot 0. It must never
  let the user believe they are editing the standalone config when they are editing
  hosted state.
- R24. Unsaved-change tracking against the loaded slot, since `0x26` reset and live edits
  do not touch flash.
- R25. Config file download and upload (`0x09`), with read-back verification because the
  upload has no acknowledgement.

### 6.6 MIDI CC map

- R26. **Resolved CC map view:** for the current link and Mixer 2 configuration, show
  exactly which CC number drives which control — e.g. "CC 8 → pan, mix 1/2, channel 1".
- R27. The map is derived from the documented rules, including the stereo cases: use the
  lower-numbered CC of a stereo pair, and pan takes the CC the right-channel mix would
  have used.
- R28. **Warn when a link change rewrites the CC map.** Toggling a stereo link silently
  changes what existing controller mappings do. The app must show what changed.
- R29. MIDI channel assignment for USB and DIN ports, including Off.
- R30. The CC map is exportable, so a controller can be mapped against it away from the
  app.

### 6.7 Protocol monitor

- R20. Live TX/RX log with hex bytes and a decoded interpretation per message. Because
  `0x34`'s index byte *is* the CC number, this doubles as the manual's own recommended
  method for identifying a control's CC — a labelled feature rather than a debugging trick.
- R21. Ability to send an arbitrary SysEx message by hand.

## 7. Non-functional requirements

- N1. The protocol codec is a standalone Rust module with no Tauri, MIDI, or UI
  dependency, unit-testable against captured byte arrays.
- N2. Application state lives in a plain data model; the DOM renders from it. No
  `document.write`, no reading values back out of DOM elements.
- N3. Protocol encode/decode is a separate layer with no DOM dependency.
- N4. Every outbound message is emitted through one function so the monitor and undo stack
  see all traffic.
- N5. Continuous controls throttle SysEx output during a drag.

## 8. Decisions

**D1 — The mixer is macro-only.** Level + pan strips are the only editable mixer. The raw
matrix is read-only: the macro layer continuously drives it, so a raw value survives only
until anything moves the corresponding macro control — including a CC from a controller,
which in the live rig is continuous. Offering a control whose value silently will not
survive normal use is worse than not offering it.

**D2 — Routing: patchbay matrix.** Chosen over a flow diagram for edit speed and over the
v1.2 grid for showing all connections simultaneously. See R15 for the one-assignment
constraint that makes it a move rather than a toggle.

**D3 — Density: console layout.** Large faders, one mix in focus at a time, with a
selector to switch mixes. Prioritises real-time adjustment over simultaneous visibility.

**D4 — Metering: host-side, from the USB capture stream.** The protocol genuinely has
none. This is what forced the platform choice: Chrome clamps `getUserMedia` audio input to
2 channels, so a browser app could never meter more than one stereo pair.

**D5 — Meter scope: inputs and mix outputs.** Capture channels are reserved as needed to
guarantee those points are visible, with the reservation made explicit in the UI (M4).

**D6 — MIDI control: resolved CC map view, not a translation layer.** The hardware's CC
numbers are fixed, so the app shows the truth rather than inventing mappings the module
will not honour. A translation layer — remapping an arbitrary controller through the
computer — is deferred: it only works laptop-attached, and would not survive the
standalone case.

**D7 — Both operating contexts are first-class.** Consequence: the standalone-config
authoring workflow (R23) carries the same weight as the live mixer, and metering is a
hosted-only enhancement rather than a core promise.

## 9. Acceptance criteria

- A1. With no device connected, the app loads, shows a clear "no ES-9 found" state, and
  the mock device can drive every view.
- A2. Connecting a device populates routing, mixer, links and options from a single config
  dump with no further user action.
- A3. Moving a fader produces exactly one `0x34` message per settled value, not one per
  pointer event.
- A4. A round trip — change state, request config, reparse — reproduces the same UI state,
  for every field in scope.
- A5. Undo restores both the UI and the device to the prior state.
- A6. Every byte sent or received appears in the protocol monitor, decoded.
- A7. A firmware reporting a config version other than 3 yields a warning and read-only
  mode, never a blank or broken page.
