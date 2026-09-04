# ES-9 Mixer

An alternative control surface for the [Expert Sleepers ES-9](https://expert-sleepers.co.uk),
built around its MIDI SysEx protocol.

Ableton can route audio to and from the ES-9 but cannot touch the module's internal gain
and routing. This drives the module's own matrix mixer directly — and, more to the point,
lets a configuration be built on the desktop and then carried into a DAW-free live rig
where a MIDI controller rides the mix.

## Status

Working and installed. `crates/es9-app` connects to the module over MIDI, shows its real
configuration, and meters sixteen channels at 48 kHz over ASIO.

The protocol layer is verified against a real ES-9 on **firmware 1.3.1** — the pan
storage, the macro/raw precedence and the chunked config upload are measured rather than
assumed. Metering runs **alongside a DAW**: the ASIO driver is multi-client in practice,
so the meters keep working with Ableton open on the same module. WASAPI is rejected, not
for latency but because shared mode re-clocks the module to whatever rate Windows has
configured for the endpoint.

Tabs: **Mixer**, **Routing** (two patchbays — sources into capture channels, then block
outputs into destinations, with cables you patch by clicking a jack at each end),
**Analogue** (input DC blocking and output DC offsets), **Meters**, **Presets**, **CC
Map** (with the stereo links that rewrite it), and **Monitor**.

Targets **firmware 1.3.x**. Firmware 1.2 used a different configuration dump format and is
detected and rejected rather than misparsed.

What is left, and what is still unverified on hardware, is in `features/mixer-ui/`.

## Layout

| Path | What it is |
|---|---|
| `crates/es9-protocol` | SysEx codec. Pure: no I/O, no platform dependencies. |
| `crates/es9-device` | Device state, actions, undo, offline mock, presentation model. Pure. |
| `crates/es9-midi` | `midir` transport, SysEx reassembly, protocol monitor, `probe` binary. |
| `crates/es9-audio` | ASIO/WASAPI capture and meter ballistics, `audioprobe` binary. GPL-3.0. |
| `crates/es9-app` | Tauri desktop shell. Its own workspace — Tauri cannot build on Linux. GPL-3.0. |
| `crates/es9-wasm` | Browser bridge driving the mock. |
| `web/` | Frontend prototype. |
| `docs/es9-sysex-protocol.md` | The protocol, from the manual's appendix and the official tool. |
| `features/mixer-ui/` | Requirements, remaining work, and the current development state. |

## Running the prototype

```sh
./build-web.sh          # compile the bridge to WASM
cd web && python3 -m http.server 8777
```

Then open <http://localhost:8777>. It runs entirely against the mock — no ES-9 required.

## Installing (Windows)

```powershell
git clone --depth 1 https://github.com/audiosdk/asio.git C:\Users\alien\asiosdk
powershell -File scripts\win-install.ps1
```

Builds a release binary, installs it to `%LOCALAPPDATA%\Programs\ES-9 Mixer` and adds a
Start Menu shortcut. Per-user, so no elevation; re-run it to upgrade. Windows does not let
applications pin themselves to the taskbar, so for that: find **ES-9 Mixer** in the Start
menu, right-click, **Pin to taskbar**.

Presets live in `%APPDATA%\es9-mixer\presets` as plain `.syx` configuration dumps, so
they interchange with anything else that speaks the protocol — including the archives the
hardware probe writes.

## Running from source (Windows)

```powershell
powershell -File scripts\win-build.ps1 -Run
```

It finds the ES-9 by sending a version request to every MIDI port pair and keeping the one
that answers — the module is published under a generic jack name, so it cannot be found by
name. Audio opens independently, so meters work even if MIDI does not, and vice versa.

> **Editing `web/` requires rebuilding the shell.** Tauri bakes the frontend into the
> binary at compile time, so re-running an old executable shows the old UI.

## Hardware probes (Windows)

The module, its ASIO driver and the MSVC toolchain are all on the Windows host; WSL2 has
no sound subsystem and no USB passthrough. Windows `cargo` builds straight from the
`\\wsl$` path, so the working copy stays here.

```powershell
git clone --depth 1 https://github.com/audiosdk/asio.git C:\Users\alien\asiosdk
powershell -File scripts\win-build.ps1 -Probe        # MIDI: dump, decode, checklist
powershell -File scripts\win-build.ps1 -AudioProbe   # audio: enumerate, open, meter
```

`probe` is read-only and archives a full dump before anything else; its write-side spikes
are behind `--write-tests`. It needs explicit `--in`/`--out` port indices, because the
ES-9 is published through Windows MIDI Services under a generic jack name that does not
contain "ES-9".

## Licence

The protocol, device, MIDI and WASM crates are MIT. `crates/es9-audio` links the Steinberg
ASIO SDK under its **GPLv3** option, so that crate and any binary linking it are GPL-3.0.

## Tests

```sh
cargo test              # workspace unit and integration tests
./scripts/smoke.sh      # end-to-end through the WASM bridge, headless
```

## Credit

The protocol was read from the ES-9 user manual's SysEx appendix and from Expert Sleepers'
own MIT-licensed configuration tool at
[expertsleepersltd/ES-9_tools](https://github.com/expertsleepersltd/ES-9_tools).
