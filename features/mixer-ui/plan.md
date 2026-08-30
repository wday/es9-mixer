# ES-9 Mixer — Plan

What is left to do. Everything already built and everything already measured is in
`devstate.md`; the path taken to get there is in git.

Derived from `requirements.md` and `../../docs/es9-sysex-protocol.md`.

## 1. Outstanding work

Ordered by how much each is worth on the rig, not by phase.

### MIDI channel assignment (R29)

`Action::SetMidiChannels` and `encode::set_midi_channels` exist; there is no UI. This
gates CC control entirely — a controller cannot reach the module until the USB or DIN
channel is set — so it is the largest remaining gap between the app and the live use case.
Both ports take a channel or Off, and DIN is the one that matters standalone.

Belongs on the CC Map tab, above the map, alongside the stereo links: the same reasoning
that put the links there.

### CC map export (R30)

The map is resolved and displayed but cannot leave the app. A controller is mapped at the
controller, away from the screen showing the map. Plain text or CSV is enough.

### Port picker (H0)

Autoconnect covers the common case — the shell version-requests every port pair and keeps
whichever answers — but there is no way to override it from the UI when it picks wrong or
finds nothing. The module cannot be identified by name, so the picker must confirm a
choice with a version handshake rather than trusting the list.

### User labels (R16)

Every source and destination takes a user label, stored locally, shown everywhere that
source appears. The presentation model already funnels all labelling through
`es9-device::view`, so this is one layer with an override table rather than a change
spread across the UI.

### Capture channel reservation for metering (M4)

Metering a point that is not already routed to a USB capture channel means spending one.
The app should show that it has done so and let the user decline, rather than quietly
rewriting the routing to draw a meter.

## 2. Next playtest

Unverified on hardware from the most recent work, in the order worth checking.

1. **The header renders as intended.** It should read `Block 3: S/PDIF` followed by a
   dimmed `out ← USB 5/6 · in not captured` (or `in → capture 15/16`, depending on the
   routing). Covered by tests through the mock but never seen drawn. Confirm you are on a
   freshly built shell, not a stale baked-in frontend.
2. **Set capture 15/16 to MIX 1 / MIX 2** on the Capture tab, then record those two
   channels in the DAW. That is the stereo reference mix of exactly what leaves the main
   outs. Check it against the multitracks for level and alignment. Note that the mixer is
   the main output volume control, so riding those faders during a take is baked into the
   recording.
3. **Loading a preset**, still never done against hardware.
4. **Factory reset**, still never done against hardware — and note it restores the factory
   routing, which puts S/PDIF back on capture 15/16 and removes the reference capture from
   step 2.

## 3. Hardware verification still outstanding

Run with `crates/es9-midi/src/bin/probe.rs`, which archives a full dump before anything
else. Write-side checks sit behind `--write-tests`.

| # | Check | Why it matters |
|---|---|---|
| H6 | Physical input permutation matches `INPUT_CAPTURE_LOOKUP` | Decodes plausibly; not confirmed by ear against what is actually patched |
| H7 | Output permutation, incl. the `Output 7 = 11` / `Output 8 = 10` swap | As H6 |
| H11 | Which physical inputs each DC bit governs | Inferred from the permutation. Needs a DC offset on one input of a pair, filter toggled, signal observed |
| — | Pan half of the macro region (`[128..256]`) | Confirmed only against an all-centre configuration. Needs a deliberately asymmetric one |
| — | Preset load and factory reset, on hardware | Covered by mock tests and the upload transport is verified, but the buttons are untried on the module |

**Before any config upload testing: save both flash slots and archive a full dump.**

## 4. Live risks

- **R-3. Firmware format drift.** Already bitten once: 1.2 → 1.3 changed the dump format
  while leaving the version field at 3. Discriminate on the command byte and fail loudly.
- **R-7. Link toggles silently rewrite the CC map.** A stereo link changes what half the
  CC numbers mean, so a controller mapping is valid for exactly one link configuration.
  The hardware behaviour cannot be changed; R28 mitigates by surfacing the diff.
- **R-8. WASAPI re-clocks the module.** Metering is ASIO-only for this reason. If a WASAPI
  fallback is ever added for machines without the driver, it must read the module's rate
  over SysEx first and refuse to open on a mismatch rather than silently re-clocking.
