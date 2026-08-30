# ES-9 SysEx Protocol

**Primary source: the official SysEx appendix in the ES-9 v1.3 user manual** (pages
15–19), cross-checked against the config tool source.

Sources, all archived under `docs/reference/`:

| File | What it is |
|---|---|
| `es9_user_manual_1.3.txt` | Official manual, incl. the authoritative SysEx appendix |
| `es9_config_tool_official_fw1.3.html` | **MIT-licensed** official tool from `expertsleepersltd/ES-9_tools`, for firmware 1.3.0 |
| `es9_config_tool_1.3.html` | Web build of the same |
| `es9_config_tool_1.2.html` | Older tool, firmware 1.2 — kept because the dump format differs |

> **The official config tool is MIT licensed** (Copyright (c) 2023 Expert Sleepers Ltd).
> It may be referenced and derived from directly.

Status: **specification-grounded, not yet hardware-verified.** The manual settles most of
what was previously guessed; the remainder is in [Open questions](#open-questions).

> ### ⚠ The config dump format changed between firmware 1.2 and 1.3
>
> Firmware 1.2 sends the dump as command `10H` with a **mixed byte/word** layout — single
> bytes for flags and routing, 3-byte groups for gains. Firmware 1.3 sends it as command
> `08H` with a **uniform array of 21-bit words**: every field, including single-bit flags,
> occupies three bytes.
>
> The internal `version` field is **still 3 in both**, so it cannot be used to tell them
> apart. Discriminate on the **command byte** (`10H` vs `08H`) instead.
>
> This document specifies the **1.3 format** as primary. The 1.2 layout is in Appendix A.

## 1. Frame

```
F0  00 21 27  19  <cmd>  [payload...]  F7
    |          |    |
    |          |    +-- command byte
    |          +------- device ID: ES-9
    +------------------ manufacturer ID: Expert Sleepers
```

The tool's receive handler matches the first five bytes exactly (`F0 00 21 27 19`) and
drops anything else, so the header is fixed — there is no device-index or channel nibble.

### Multi-byte encoding

MIDI data bytes are 7-bit, so wider values are split big-endian, most significant first:

| Width | Encoding | Used by |
|---|---|---|
| 21-bit | `v>>14, (v>>7)&0x7F, v&0x7F` | mixer gain, DC offset, EQ freq/Q/gain, smoothing |
| 14-bit | `v>>7, v&0x7F` | DSP usage counters |

Signed 21-bit fields (DC offset, EQ gain) are sign-extended from 16 bits on receive:
`v = (v << 16) >> 16`.

## 2. Device model

The ES-9 exposes **four routing blocks of 8 channels each**, referred to as "DSP" blocks.
Each block has a *capture* side (what feeds it) and an *output* side (where it goes).

| Block | Capture side (`0x40+dsp`) | Output side (`0x50+dsp`) |
|---|---|---|
| 0 | USB in 1–8 sources | USB out 1–8 destinations |
| 1 | USB in 9–16 sources | USB out 9–16 destinations |
| 2 | Mixer bank A: 8 channel sources | Destinations for mixes 1–8 |
| 3 | Mixer bank B: 8 channel sources | Destinations for mixes 9–16 |

Block 3 is dual-purpose. Options bit 0 selects its role:

- **bit 0 clear** — channels 2–7 are the S/PDIF ports; mixer bank B is unavailable
  (mixes 9–16 hidden, EQ bank 1 hidden).
- **bit 0 set** — "Mixer 2": block 3 is a second full mixer bank.

### The mixer

The manual describes each mixer block as an **8x8 crosspoint mixer**: 8 inputs, 8
outputs. Each of the 8 outputs is one "mix". Two blocks therefore give 16 mixes x 8
channels = **128 gain cells**. Mixes 0–7 draw from block 2's capture sources, mixes 8–15
from block 3's. Every mix in a bank sees the *same* 8 sources.

The manual also confirms the surrounding structure:

- USB audio block: **16 inputs and 16 outputs**
- **16 internal mono summing buses** for passing audio between blocks (bus 16 is used as a
  dummy/dead-end destination in the default config)

Two representations of the same matrix exist, both live on the device:

- **Raw mix** — one 21-bit linear gain per cell. Written with `0x60+mix`.
- **Virtual mix** — a 7-bit level plus a 7-bit pan per cell, which the *device* expands
  into raw gains. Written with `0x34`.

The virtual layer is device-side, not tool-side: the mix dump (`0x11`) returns both
representations, and the tool never computes raw values from level/pan.

### Modes and flash slots

The ES-9 holds **two** configurations in flash, and which one is live depends on how it
powered up:

| Slot | Loaded when |
|---|---|
| **standalone** | the module powers up with **no USB connection** |
| **hosted** | there **is** a USB connection to a host |

Behaviours that matter for any tool that edits these:

- The hosted config is auto-loaded **only on the first USB connection** after power-up.
  This is deliberate: if the cable is knocked out or the host crashes mid-set, the module
  does not thrash between configurations.
- **Connecting a config tool over USB puts the module in hosted mode.** To edit the
  *standalone* configuration you must first `0x25` restore-from-flash slot 0, edit, then
  `0x24` save-to-flash slot 0. Editing "the standalone config" without that restore step
  silently edits the hosted state instead. This is the single easiest way for a user to
  lose work, and the reference tool does nothing to prevent it.
- `0x26` reset-to-defaults affects only the **current** state, not either flash slot, and
  it **takes a parameter**: `1` selects hosted-appropriate defaults (mixer inputs = USB
  channels, S/PDIF enabled), `0` standalone-appropriate (mixer inputs = analogue inputs,
  Mixer 2 enabled). The command table in §4 omitted the parameter and called it a flash
  reset; both were wrong. The two default sets are the module's own, not the tool's — the
  tool just sends `26H` with the byte.
- Holding the headphone knob for over a second toggles which flash config is loaded. LED C
  indicates hosted, LED D standalone.
- The MIDI Thru option (`0x32` bit 1) applies **only in standalone mode**.

### Stereo links

A 32-bit link bitmap marks source pairs as stereo. When a pair is linked, the UI merges
the two mono strips into one stereo strip with a pan control, and changing one channel's
capture source sets its partner to the adjacent source.

| Bits | Meaning |
|---|---|
| 0–6 | links for source group `0x70` (physical inputs), pairs 0/2/4/…/12 |
| 8–15 | links for source group `0x60` (buses), pairs 0/2/…/14 |
| 16–19 | links for group `0x00` (USB 1–8) |
| 20–23 | links for group `0x10` (USB 9–16) |
| 24–27 | links for group `0x20` (MIX) |
| 28–31 | links for group `0x30` (S/PDIF) |

The link index used in `0x33` is `which` = the checkbox id `link<group>_<pair>`, i.e.
group nibble and even channel number.

## 3. Commands: host to ES-9

| Cmd | Payload | Meaning |
|---|---|---|
| `0x02` | ASCII text | Send a text message to the module |
| `0x09` | `chunk`, `0`, 32 words | Upload one chunk of a config dump (24 chunks x 32 words) |
| `0x22` | — | Request version string |
| `0x23` | — | Request config dump |
| `0x24` | `wh` | Save to flash (`1` = hosted, `0` = standalone) |
| `0x25` | `wh` | Restore from flash |
| `0x26` | `wh` | Reset the **current** configuration to defaults (`1` = hosted, `0` = standalone) |
| `0x28` | `wh`, `1-v` | Codec reset, `wh` = codec index (0-based) |
| `0x29` | `wh` | Codec resync |
| `0x2A` | — | Request mix dump |
| `0x2B` | — | Request DSP usage |
| `0x2C` | — | Request sample rate |
| `0x31` | `b` | Input DC blocking (HPF) bitmap, bits 0–6 = the seven input **pairs** below |
| `0x32` | `options` | Options: bit0 = Mixer 2 enable, bit1 = MIDI thru |
| `0x33` | `which`, `on` | Set stereo link |
| `0x34` | `mix`, `level` | Macro mix fader — manual documents **level only** |
| `0x35` | `usb`, `din` | MIDI channels for USB and DIN |
| `0x36` | `ch`, 21-bit | Output DC offset for channel `ch` (signed) |
| `0x39` | `8*m+ch`, `f`, `type`, freq21, Q21, gain21 | EQ band `f` (0–3) on bank `m`, channel `ch` |
| `0x3A` | `which`, `on` | Gain smoothing enable, `which` = mix 0–15 |
| `0x40+dsp` | 8 source bytes | Set capture routing for block `dsp` |
| `0x50+dsp` | 8 destination bytes | Set output routing for block `dsp` |
| `0x60+mix` | `ch`, 21-bit | Raw mixer gain for mix `mix`, channel `ch` |

### `0x34` addressing — the index *is* a CC number

The manual documents this as **`34H – Set Virtual Mix: <mix> <level>`**, and then, in the
MIDI CC section, states plainly:

> *"The byte that follows the '34' is the CC number that corresponds to the control you
> just moved."*

So the macro mix parameter space and the MIDI CC space are **the same 128-entry space**.
`0x34 <cc> <value>` writes the same parameter that CC `<cc>` would. There is no separate
pan command because pan is simply another entry in that space. See §5a for the mapping.

This resolves what was open question Q3: pan for a stereo pair `(m, m+1)` lives at index
`(m+1)*8 + ch`. The config tool's **write** is correct; its **read** is the buggy half.

## 4. Commands: ES-9 to host

| Cmd | Payload | Meaning |
|---|---|---|
| `0x08` | `00 00` + 768 words | Full config dump (**firmware 1.3**) |
| `0x10` | `00 00` + mixed layout | Full config dump (**firmware 1.2**, see Appendix A) |
| `0x11` | 128 words + 256 bytes | Full mix state |
| `0x12` | 4 x 14-bit | DSP usage |
| `0x14` | 21-bit | Sample rate / 4 |
| `0x32` | NUL-terminated ASCII | **Message** |
| `0x60+mix` | `ch`, 21-bit | Raw mixer gain changed |

### `0x32` — Message

The manual is explicit that this is a **general-purpose string response**, not just a
version string: *"transmitted in response to any request for a string e.g. the version
string, and also to give feedback on the success or otherwise of an operation e.g. saving
the state to flash."* A flash save (`0x24`) replies with `"OK"`.

The string is **NUL-terminated**. The 1.3 tool slices `data.slice(6, -2)` to drop both the
NUL and the `F7`; the 1.2 tool sliced `(6, -1)` and left a trailing NUL in the string.
Strip the NUL.

Note `0x32` is **Set Options** outbound and **Message** inbound — same byte, different
meaning by direction.

### Config dump (`0x08`, firmware 1.3)

```
F0 00 21 27 19 08 00 00 <768 x 21-bit word> F7
```

Every field is a 21-bit word (3 bytes, big-endian 7-bit groups) — including single-bit
flags. Total message length: `8 + 768*3 + 1` = **2313 bytes**.

The tool derives the word count as `(length - 8 - 1) / 3`, so it tolerates a short dump;
768 is what the upload path assumes.

| Word | Count | Field |
|---|---|---|
| 0 | 1 | version — must be `3` |
| 1 | 1 | input DC blocking bitmap (bits 0–6) |
| 2 | 32 | capture routing, 4 blocks x 8 channels |
| 34 | 32 | output routing, 4 blocks x 8 channels |
| 66 | 128 | raw mix, 16 mixes x 8 channels |
| 194 | 256 | macro mix — **skipped by the official tool** (see below) |
| 450 | 1 | options |
| 451 | 1 | links, low 16 bits |
| 452 | 1 | links, high 16 bits |
| 453 | 1 | MIDI channels, packed: `usb = (w >> 8) & 0xFF`, `din = w & 0xFF` |
| 454 | 8 | output DC offsets, signed 16-bit |
| 462 | 256 | EQ: 2 banks x 8 ch x 4 bands x (type, freq, Q, gain) |
| 718 | 1 | gain smoothing bitmap, bits 0–15 |
| 719 | 49 | unused / reserved, padding to 768 |

Two things to note:

- **The MIDI channel packing changed.** Firmware 1.2 used two separate bytes; 1.3 packs
  both into one word as two 8-bit halves.
- **The official tool skips the 256 macro-mix words** (`c += 2 * 128`), relying on a
  separate `0x2A` request instead. So the meaning of those 256 words — whether they are
  128 levels followed by 128 pans, or 128 interleaved pairs — is **not established by the
  reference implementation**. See open question Q1.

### Config upload (`0x09`)

The dump is sent back in **24 chunks of 32 words** each:

```
F0 00 21 27 19 09 <chunk 0-23> 00 <32 x 21-bit word> F7
```

Each chunk reuses bytes 0–7 of the received dump with byte 5 replaced by `09` and byte 6
by the chunk index. Chunk length: `8 + 96 + 1` = 105 bytes. There is **no documented
acknowledgement or flow control** — the tool fires all 24 chunks in a loop.

### Mix dump (`0x11`)

```
F0 00 21 27 19 11 <128 x 3 bytes of raw mix> <128 x macro mix/pan> F7
```

Payload: 384 + 256 = **640 bytes**. The macro section is 128 x (level, pan). Pan is read
back biased by **63**, while `0x34` writes it biased by **64** — see Q2.

### Usage (`0x12`)

Four 14-bit counters: `u0, u1` for DSP 0 and `u2, u3` for DSP 1 (`u2 == 0` means the
second DSP is inactive). Load percentage:

```
pct = (u1 - u0) * 100 / (4096 - u0)
```

### Sample rate (`0x14`)

`rate = 21-bit value * 4` Hz.

## 5. Value scales

### Raw mixer gain (16-bit, transmitted as 21 bits)

```
dB    = 6 * log2(v / 8192)      unity at 0x2000, +12 dB at 0x7FFF, 0 = -inf
v(dB) = round(0x2000 * 2^(dB/6)), clamped to 0x7FFF, 0 below -72 dB
```

### Macro mix level (7-bit, piecewise)

```
v = 0        -> -inf
v in 1..54   -> dB = -72 + (v - 1) * 48 / 54        (coarse, ~0.89 dB/step)
v >= 55      -> dB = -24 + 0.5 * (v - 55)           (fine, 0.5 dB/step)
```

`v = 103` is the double-click default (0 dB). Max `v = 127` gives +12 dB.

### Macro mix pan

`-63` (hard left) to `+63` (hard right), `0` = centre.

### EQ

```
freq = 10.0 * exp(log(22000/10) / 32767 * v)     Hz,  v = 0..32767
Q    = 0.1  * exp(log(18/0.1)  / 32767 * v)            v = 0..32767
gain = v * 15 / 32767                            dB,   type 5 clamps to +12 dB
```

Type byte: `(type << 1) | enabled`. Types with a gain control: 4, 5, 6. Types with a Q
control: 2, 3, 6.

## 5a. MIDI CC control of the macro mix

The ES-9 can be driven directly by MIDI CC, **in hardware, with no computer involved**.
This is enabled per-port in the "Mixer MIDI channels" settings (`0x35`): separate MIDI
channels for the USB and DIN ports, or `Off` to disable. CC control addresses the **macro
mix, not the raw mix**.

### The mapping

CC numbers are **fixed** — not user-assignable:

```
CC = mix_index * 8 + channel_index
```

incrementing first by channel, then by mix. Mixes 1–8 occupy CC 0–63; with Mixer 2
enabled, mixes 9–16 occupy CC 64–127.

| Control | CC |
|---|---|
| Mix 1, channel 1 | 0 |
| Mix 1, channel 8 | 7 |
| Mix 2, channel 1 | 8 |
| Mix 8, channel 8 | 63 |
| Mix 9, channel 1 | 64 |
| Mix 16, channel 8 | 127 |

### Stereo changes the mapping

This is the part that makes a CC map view worth building — the rules are not obvious:

- **When a mixer input or a mix is stereo, always use the lower-numbered CC of the two
  that would otherwise apply.** If mix 1/2 is stereo, mixer channel 1 uses CC 0, not CC 8.
  If USB 1/2 is stereo and they are inputs 1 and 2 of mixer 1, use CC 0, not CC 1.
- **Pan, on a stereo mix, takes the CC that the right-channel mix would have used.** With
  mix 1/2 stereo, pan for channel 1 is CC 8.

So enabling a stereo link does not merely merge two faders in the UI — it **silently
reassigns what half the CC numbers mean**. Any controller mapping is only valid for a
given link configuration.

### Consequence for the app

Because the CC numbers are fixed and the stereo rules are subtle, the useful thing an app
can do is **show the resolved CC map for the current configuration** — "CC 8 currently
controls pan on mix 1/2 channel 1" — rather than let the user assign mappings that the
hardware will not honour.

The manual's own advice for working this out is to watch the SysEx: operate a control in
the config tool and read the byte after `34`. A protocol monitor makes this a first-class
feature rather than a debugging trick.

## 6. Routing source and destination values

### Capture sources (`0x40+dsp` payload bytes)

| Value | Meaning |
|---|---|
| `0x00`–`0x07` | USB 1–8 |
| `0x10`–`0x17` | USB 9–16 |
| `0x20`–`0x27` | MIX 1–8 |
| `0x30`, `0x31` | S/PDIF L, R |
| `0x60`–`0x6F` | Bus 1–16 |
| `0x70`–`0x7F` | Physical inputs — **permuted**, see below |

Physical inputs are not in wire order. The tool keeps a lookup table and translates in
both directions:

```js
inputCaptureLookup = [0x78, 0x79, 0x76, 0x75, 0x74, 0x7B,
                      0x7A, 0x77, 0x73, 0x72, 0x7D, 0x7C, 0x7E, 0x7F]
```

Index = Input N-1, value = wire byte. On receive, a byte with high nibble `7` is mapped
back via `indexOf`. Only 14 inputs are offered although the range holds 16.

### Output destinations (`0x50+dsp` payload bytes)

| Value | Meaning |
|---|---|
| `6`, `7` | Main Out L, R |
| `12`, `13` | Phones L, R |
| `14`, `15` | ES-5 L, R |
| `8`, `9` | Output 1, 2 |
| `4`, `5` | Output 3, 4 |
| `2`, `3` | Output 5, 6 |
| `11`, `10` | Output 7, 8 |
| `0x10`–`0x1F` | Bus 1–16 |

Physical outputs are also permuted, and note `Output 7 = 11` / `Output 8 = 10` is a
deliberate swap. Values `0`, `1` are unused by the UI.

## 7. Open questions

Most of the original guesses were settled by the manual. What remains:

**~~Q1 — Macro-mix words in the config dump.~~ RESOLVED on hardware 2026-08-30.** The 256
words are **two parallel arrays of 128 indexed by MIDI CC**: `[0..128]` levels,
`[128..256]` pans — not interleaved pairs. Words 0 and 9 read 103 while the mix dump
independently reported CC 0 and CC 9 at 103, and all 128 words of the high half read
exactly 64, the pan centre. Confirmed by writing: `0x34` CC 0 = 103 then re-reading the
dump gives `macro_words[0] == 103`.

The official tool's `0x2A` re-request is therefore unnecessary — the configuration dump
alone carries the macro mix. Pan values remain written verbatim until a deliberately
asymmetric pan configuration confirms the high half the same way.

**~~Q2 — Pan bias off-by-one.~~ RESOLVED on hardware 2026-08-30 — and this project's
first answer was wrong.** Both biases are correct. The module stores `written - 1`, so a
written centre of 64 is stored as 63 and read back with `- 63`. The earlier conclusion,
that resting `aux = 64` proved centre was 64, mistook the module's untouched default (one
step right of centre) for centre itself. See §11 for the measured sweep.

**~~Q3 — Pan row.~~ RESOLVED by the manual, then corrected on hardware 2026-08-30.** Pan
for a stereo pair `(m, m+1)` is *written* to index `(m+1)*8 + ch` — the CC the
right-channel mix would have used. But it is not *stored* there: the module keeps it in
the aux byte of the level cell, `m*8 + ch`. Both halves of the reference tool are right.
See §11.

**~~Q4 — Config upload acknowledgement.~~ RESOLVED on hardware 2026-08-30.** There is no
acknowledgement, and none is needed: all 24 chunks were sent back to back with **no pacing
whatsoever** and the configuration read back byte-identical. A caller still has to verify
by re-reading, since nothing in the protocol would report a failure, but the transport
itself is reliable at full speed.

**~~Q5 — Raw vs macro precedence.~~ RESOLVED on hardware 2026-08-30. Macro drives raw.**
A raw write *does* stick: `0x60+mix` with `0x4000` read back as `0x4000`. But the moment
the macro layer touches that cell, the raw value is replaced — a `0x34` write to the same
cell turned it into the raw equivalent of the macro level.

So a raw value is durable only until anything moves the corresponding macro control,
including a MIDI CC arriving from a controller. Exposing the raw matrix as an ordinary
control would be misleading, because the value silently will not survive normal use.

**~~Q6 — Echo behaviour.~~ PARTLY RESOLVED on hardware 2026-08-30. The device does talk
back, and with more than an echo.** A single `0x34` macro write drew an unsolicited
**full `0x11` mix dump** in reply, carrying the written value.

Two consequences. A host needs a feedback guard, since its own write returns as state.
And a fader drag emits a large dump per step, so send coalescing is a bandwidth
requirement rather than a nicety. Whether routing and option changes are echoed the same
way is still unknown.

**Q7 — Reserved dump words.** Words 719–767 are unused by the tool. Observed all-zero on
hardware, which is consistent with "unused" but does not prove it.

**Q8 — Undocumented commands.** The manual lists a complete command set, so the gaps are
likely genuinely unused rather than secret. Low priority.

**Q9 — The macro → raw curve is not exactly the documented one.** *(new, 2026-08-30)*
Since macro drives raw (Q5), the module converts a macro level into a raw gain
internally. Composing the documented curves — `db_to_raw(macro_to_db(v))` — reproduces
unity exactly (macro 103 → `0x2000`, matching hardware) but is slightly off elsewhere:
macro 100 computes 6889 where a real module settled at 6832, about 0.07 dB out.

Nothing depends on this — the module does the real conversion and reports the result, so
the host never needs to predict it. It matters only for the fidelity of the offline mock,
and for knowing that the published macro curve is an approximation of what the firmware
runs. *Test:* sweep macro 0–127, read the raw matrix at each step, and compare against the
piecewise table.

### Settled by the manual

| Was | Now |
|---|---|
| Two unknown bytes after the dump command | Documented as literal `00 00` |
| `0x32` inbound "version string" | General **Message**, NUL-terminated, also used for operation feedback |
| Raw gain "21-bit" | **16-bit**, transmitted in three 7-bit bytes |
| "Virtual mix" | Official name is **macro mix** |
| Mixer shape unclear | **8x8 crosspoint** per block; USB block is 16x16; 16 summing buses |
| Unknown whether more commands exist | Manual gives a complete list — `0x09` upload was the only one missing |
| Pan addressing (Q3) | `0x34`'s index **is** a MIDI CC number; pan sits at `(m+1)*8+ch` |

## 8. Metering

**There is no metering in the ES-9's SysEx protocol.** This is confirmed by three
independent sources:

1. The manual's SysEx appendix, which is a complete command list.
2. The firmware 1.2 config tool.
3. The MIT-licensed official config tool for firmware 1.3.

The only telemetry the device reports is **DSP load** (`0x12`), which is a processing-cost
figure, not an audio level.

Metering must therefore be derived **host-side, from the USB audio stream**. This is
viable in principle because the ES-9's routing can send almost anything — physical inputs,
buses, or mixer outputs — to one of the 16 USB capture channels, so the host can observe
any point in the signal path it is willing to spend a capture channel on.

The constraint is the host API:

| Host API | Channels available | Verdict |
|---|---|---|
| Browser `getUserMedia` (Chrome) | **2** | Chrome clamps audio input to stereo. Requesting `channelCount > 2` raises `OverconstrainedError`. Open since 2015. |
| Browser `getUserMedia` (Firefox) | 2 | Same limitation, tracked in bugzilla 1393401 |
| Native (CoreAudio / WASAPI / ALSA / JACK) | 16 | Full access |

So a browser-only app can meter **one stereo pair at a time, at most**. Full 16-channel
metering requires a native audio path.

## 9. Defects observed in the reference tools

Recorded so the reimplementation does not inherit them.

- `send()` calls `makeSysEx()`, which is **not defined anywhere** in the file. The "send
  raw SysEx" button throws.
- ~~Pan round-trip is broken as described above.~~ **RETRACTED 2026-08-30.** This project
  recorded the tool as buggy here on a reading of its source alone. Hardware says
  otherwise: pan is written to CC `(m+1)*8 + ch` but the module stores it in the **aux
  byte of the level cell**, as `written - 1`. So writing `64 + pan` and reading
  `byte - 63` compose *exactly*, and the tool is right. See §11.
- ~~Pan bias is inconsistent between send and receive.~~ **RETRACTED** — same reason. The
  two biases differ by one because the device subtracts one in between, not because
  either half is wrong.
- `parseConfigDump` `alert()`s and aborts on any version but `3`, with no fallback — and
  since the `version` field is `3` in **both** the 1.2 and 1.3 dump formats, this check
  cannot actually detect the format change it appears to guard against.
- `updateMixChTag` builds a stereo tag as `tag + "/" + ((cs & 0xe) + 2)`, appending a raw
  number rather than the partner channel's label.
- The 1.2 tool leaves the trailing NUL on message strings (`slice(6, -1)`); fixed in 1.3.
- `uploadConfig` (1.3) sends 24 SysEx chunks in a tight loop with no pacing or
  acknowledgement. Whether this is reliable is untested — see Q4.

## 10. Input DC blocking pairs

The fourteen analogue inputs are DC coupled. Each ADC has an optional DC-blocking
high-pass filter, set by the bitmap in `0x31` and by word 1 of the configuration dump.

The filters switch **in pairs**, so there are seven bits, not fourteen. The manual states
this ("switched on or off in pairs", p. 9) but does not say which inputs pair up. The
official tool's labels do:

| Bit | Inputs | Wire bytes |
|---|---|---|
| 0 | 1 / 2 | `0x78`, `0x79` |
| 1 | **3 / 8** | `0x76`, `0x77` |
| 2 | 4 / 5 | `0x74`, `0x75` |
| 3 | 6 / 7 | `0x7B`, `0x7A` |
| 4 | 9 / 10 | `0x73`, `0x72` |
| 5 | 11 / 12 | `0x7D`, `0x7C` |
| 6 | 13 / 14 | `0x7E`, `0x7F` |

Bit 1 covering inputs **3 and 8** looks like a typo in the tool. It is not. A filter
covers two adjacent ADC channels, and input numbers are permuted on the wire by
`inputCaptureLookup` (§ above): mapping each pair through that table gives two wire bytes
differing only in the low bit, for all seven pairs. `crates/es9-protocol/tests/dcblock.rs`
asserts exactly that, so the table cannot be "tidied" into sequential pairs without the
test failing.

Verified on hardware 2026-08-30: walking a single bit through the field round-tripped for
all seven bits, and writing `0xFF` read back as `0x7F`, confirming a seven-bit field.

**Which physical inputs a bit governs is still inferred**, not measured — confirming it
needs a DC offset on one input of a pair and a look at the captured signal with the filter
toggled. The pairing is corroborated by the permutation, which is strong, but it is not
the same as having heard it.

The practical consequence, and the reason this is worth documenting carefully: the filter
must be **off** to use an input for CV, because it removes exactly the constant offset a
CV consists of. Someone turning the filter on for audio on input 8 also turns it on for
input 3, which would silently ruin a CV patched there.

## 11. Where pan actually lives

Measured on hardware 2026-08-30 (checklist H3), because no document settles it and the
reference tool's apparent self-contradiction turned out to be the device's behaviour.

**Pan is written to one address and stored at another.**

- Write: `0x34` with CC `(m+1)*8 + ch`, value `64 + pan`, for a stereo pair `(m, m+1)`.
- Store: the module puts `written - 1` (saturating at 0) into the **aux byte of the level
  cell**, CC `m*8 + ch`. The value of the CC written to does not change.
- Read: `aux - 63`.

Measured sweep, writing to CC 8 and reading the aux of CC 0:

| written | 0 | 1 | 32 | 63 | 64 | 65 | 96 | 126 | 127 |
|---|---|---|---|---|---|---|---|---|---|
| stored | 0 | 0 | 31 | 62 | 63 | 64 | 95 | 125 | 126 |

Every step loses one except `0`, which saturates. So the **written** centre is 64 and the
**stored** centre is 63, and code needs both constants rather than one.

This also explains the config dump's macro region: the high half (`[128..256]`, one word
per CC) is the stored aux, which is why every word of it reads 64 on a module whose pans
have never been touched — that is one step right of centre, not centre.

**The failure this causes if you get it wrong:** reading pan from the value of the CC it
was written to yields 0 for every strip, which renders as hard left across the whole
console. That is what this project's own UI did until the behaviour was measured.

## Appendix A — Firmware 1.2 config dump (`0x10`)

Retained for reference. **Do not implement against this** unless supporting firmware 1.2.
Mixed byte and word layout; offsets are in **bytes** after skipping the 8-byte prefix.

| Offset | Size | Field |
|---|---|---|
| 0 | 1 | version (`3`) |
| 1 | 1 | input DC blocking bitmap |
| 2 | 32 | capture routing |
| 34 | 32 | output routing |
| 66 | 384 | raw mix, 128 x 3 bytes |
| 450 | 1 | options |
| 451 | 8 | links, 8 nibbles |
| 459 | 1 | MIDI channel (USB) |
| 460 | 1 | MIDI channel (DIN) |
| 461 | 24 | output DC offsets, 8 x 3 bytes |
| 485 | 640 | EQ |
| 1125 | 3 | smoothing |

Payload: 1128 bytes. Differences from 1.3: flags and routing are single bytes rather than
words; links are 8 nibbles rather than 2 words; MIDI channels are two bytes rather than
one packed word; the macro mix is absent entirely.
