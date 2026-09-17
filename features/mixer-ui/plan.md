# ES-9 Mixer — Plan

What is left to do. Everything already built and everything already measured is in
`devstate.md`; the path taken to get there is in git.

Derived from `requirements.md` and `../../docs/es9-sysex-protocol.md`.

## 1. Outstanding work

Ordered by how much each is worth on the rig, not by phase.

### Patchbay: the elaborate pass

The bay is deliberately plain for a first iteration — flat rails, flat cables, flat jacks
— but every part of it is SVG built from a layout model, so the elaboration is decoration
over a structure that already exists rather than a rewrite. In rough order of how much
each adds per unit of work:

- **Cables that read as cables.** A highlight along the top of each core, a soft shadow
  cast on the panel behind, and a plug body at each end rather than a filled circle.
- **A panel that reads as hardware.** Brushed vertical gradient on the rails, screws at
  the ends, engraved rather than painted family bands, a bevel on each bezel.
- **Drag to patch**, alongside the click-click that is there now. The gesture is what a
  patchbay invites, and grabbing a plug and moving it says "one plug per jack" more
  plainly than any highlight does. Click-click stays: it is the keyboard-reachable path.
- **Cable routing that avoids collisions.** Thirty-two cables through one gap currently
  overlap wherever their spans do. Fanning the departure angle by index, or nudging the
  belly depth per cable, separates them without changing what any cable means.
- **Motion.** A new cable settling into its hang, and the dimming easing rather than
  snapping.

Two things worth deciding before the elaborate pass rather than during it: whether the
bay stays a flat front elevation or gains perspective, and whether cable colour keeps
meaning signal family once cables look real enough that colour reads as a physical cable
the user chose.

### Patchbay: automated coverage

`web/patchbay.js` has none in the repo. Its geometry, hit testing and write path were
exercised headlessly against the mock during development — jack and cable counts, the
family bands, patching from either end, the no-op when a connection already exists — but
that harness needs `jsdom`, and the project has no npm dependency at all today. The
choice is to accept one (a `package.json`, `node_modules`, and the harness moved into
`web/`) or to leave the bay covered only by looking at it. `scripts/smoke.sh` is
unaffected either way: it drives the WASM bridge and never touches the DOM.

### Mixer console: render in place

Every refresh rebuilds the whole console with `innerHTML = ''`, so keyboard focus on a
fader is lost every second to the background poll, and a drag would be lost too if
refreshes were not now deferred until the pointer comes up. Deferring is the right fix for
the drag; it is a workaround for the rebuild. Updating the existing strips in place —
values, readouts, meters — removes the class rather than the instance, and lets the
console stay live during a drag instead of frozen.

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

The header now says whether there is a link and offers Reconnect, which re-runs the same
sweep. What is still missing is choosing the pair by hand when the sweep picks the wrong
one or finds nothing repeatably — another device answering a version request first, say.
The `ports` and `connect` commands exist and are unused; the module cannot be identified
by name, so the picker must confirm a choice with a version handshake rather than
trusting the list.

### User labels (R16)

Every source and destination takes a user label, stored locally, shown everywhere that
source appears. The presentation model already funnels all labelling through
`es9-device::view`, so this is one layer with an override table rather than a change
spread across the UI.

### A distributable build

There is no installer and no prebuilt binary: installing means MSVC, LLVM and the ASIO
SDK, which is most of the audience gone. The GPLv3 option the SDK is taken under is what
makes distributing a binary legitimate, but the SDK itself is not redistributable, so no
CI runner can build the shell — a release is a local Windows build uploaded by hand.
`tauri.conf.json` has `bundle.active: false`; turning it on produces an MSI or NSIS
installer instead of the bare `.exe` that `win-install.ps1` copies into place.

Unsigned either way. Windows SmartScreen will warn on first run, and the README should say
so plainly rather than let it look like a malware hit.

### Capture channel reservation for metering (M4)

Metering a point that is not already routed to a USB capture channel means spending one.
The app should show that it has done so and let the user decline, rather than quietly
rewriting the routing to draw a meter.

## 2. Next playtest

Unverified on hardware from the most recent work, in the order worth checking.

1. **Fader drags, on the rig.** This is the thing that was wrong: a drag used to back up
   behind the pointer and a click stuck where a slide did not. Both halves are fixed —
   the model no longer lets a trailing mix dump undo a newer write, and nothing re-renders
   while the pointer is down — but neither has been felt on hardware, where the round trip
   is real. Drag a fader hard and fast, end to end, and check it tracks the pointer and
   settles on the value released at. Then do the same to a **pan**, which had no
   optimistic local update at all and moved only when the module answered. Both fixes are
   covered by tests through the mock, and the mock does reproduce the echo and the pan
   storage quirk, but it has no latency.
2. **The routing patchbay, under a pointer.** It renders correctly — see
   `docs/img/routing.png`, captured from headless Chromium at 1280×780 — so the geometry
   and the fit are settled. What is left needs a hand on it: whether thirty-two cables in
   one gap are followable, and whether pointing at a jack dims the rest enough to trace a
   single run. Then patch something and confirm the module follows: click a source, click
   a capture channel, and watch the `40H`/`50H` write in the Monitor tab. Worth doing in
   the shell rather than a browser, since the WebView is what ships.
3. **Pull the USB cable while the app is open.** The header should go to **not
   connected**, Save/Undo/Redo should disable, and any control you touch should raise the
   notice rather than moving. Plug it back in, press **Reconnect**, and it should find
   the module again — the sweep has to be able to reopen a port the dropped link held.
4. **The header renders as intended.** It should read `Block 3: S/PDIF` followed by a
   dimmed `out ← USB 5/6 · in not captured` (or `in → capture 15/16`, depending on the
   routing). Covered by tests through the mock, and the Mixer 2 variant renders (it is in
   `docs/img/routing.png`), but the S/PDIF wording has never been seen drawn. Confirm you
   are on a freshly built shell, not a stale baked-in frontend.
5. **Set capture 15/16 to MIX 1 / MIX 2** in the capture bay, then record those two
   channels in the DAW. That is the stereo reference mix of exactly what leaves the main
   outs. Check it against the multitracks for level and alignment. Note that the mixer is
   the main output volume control, so riding those faders during a take is baked into the
   recording.
6. **Loading a preset**, still never done against hardware. Save the current
   configuration first. Note that the factory reset restores the factory routing, which
   puts S/PDIF back on capture 15/16 and removes the reference capture from step 5.

## 3. Hardware verification still outstanding

Run with `crates/es9-midi/src/bin/probe.rs`, which archives a full dump before anything
else. Write-side checks sit behind `--write-tests`.

| # | Check | Why it matters |
|---|---|---|
| H6 | Physical input permutation matches `INPUT_CAPTURE_LOOKUP` | Decodes plausibly; not confirmed by ear against what is actually patched |
| H7 | Output permutation, incl. the `Output 7 = 11` / `Output 8 = 10` swap | As H6 |
| H11 | Which physical inputs each DC bit governs | Inferred from the permutation. Needs a DC offset on one input of a pair, filter toggled, signal observed |
| — | Pan half of the macro region (`[128..256]`) | Confirmed only against an all-centre configuration. Needs a deliberately asymmetric one |
| — | Preset load, on hardware | Covered by mock tests and the upload transport is verified, but the button is untried on the module. The factory reset has been used on the rig and behaves as expected |

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
