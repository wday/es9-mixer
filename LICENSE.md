# Licence

This repository is split deliberately. The boundary is the audio backend and nothing
else, so that everything needed to speak to an ES-9 and draw its meters can be reused
under a permissive licence.

| Path | Licence | Why |
|---|---|---|
| `crates/es9-protocol` | MIT | Pure codec. No I/O, no platform dependency. |
| `crates/es9-device` | MIT | State, actions, undo, mock, presentation model. Pure. |
| `crates/es9-midi` | MIT | `midir` transport. Links nothing encumbered. |
| `crates/es9-meter` | MIT | Meter ballistics. No backend, no I/O. |
| `crates/es9-wasm` | MIT | Browser bridge over the mock. |
| `web/` | MIT | Frontend. |
| `docs/`, `features/` | MIT | Prose written for this project. |
| **`crates/es9-audio`** | **GPL-3.0-only** | Links the Steinberg ASIO® SDK under its GPLv3 option. |
| **`crates/es9-app`** | **GPL-3.0-only** | Links `es9-audio`. |

Full texts: [`LICENSE-MIT`](LICENSE-MIT), [`LICENSE-GPL-3.0`](LICENSE-GPL-3.0).

## What this means in practice

**Using the desktop application**, or any binary built from `crates/es9-app`, puts you
under the GPLv3. That is the licence the Steinberg ASIO SDK is taken under here; the
proprietary option requires a separate agreement with Steinberg.

**Building something else on the core** — a different UI, another platform, a host that
cannot carry GPLv3 code at all — needs only the MIT crates. They link nothing encumbered.
Supply your own capture backend and the whole protocol, device model and metering stack
comes with you. This is why the ballistics live in `es9-meter` rather than next to the
capture code that feeds them; moving them back would drag GPLv3 across a boundary drawn
on purpose.

**The ASIO SDK itself is not redistributed here** and is not a dependency you can fetch
from crates.io. It is downloaded separately at build time and pointed at with
`CPAL_ASIO_DIR`.

## Third-party material

The SysEx protocol in `docs/es9-sysex-protocol.md` was read from the ES-9 user manual's
appendix and from Expert Sleepers' own MIT-licensed configuration tool
([expertsleepersltd/ES-9_tools](https://github.com/expertsleepersltd/ES-9_tools),
Copyright (c) 2023 Expert Sleepers Ltd). The document is this project's own writing; the
sources it was read from are not redistributed here. See
[`docs/reference/README.md`](docs/reference/README.md).

ASIO is a registered trademark of Steinberg Media Technologies GmbH. The ASIO SDK is
dual-licensed, proprietary or GPLv3; this project takes the GPLv3 option, which is what
makes both the source and a compiled binary distributable. The SDK is compiled in as
object code and is not redistributed here in any form.

This project is not affiliated with or endorsed by Expert Sleepers Ltd or Steinberg Media
Technologies GmbH.
