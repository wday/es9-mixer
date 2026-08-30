//! Round-trip and framing tests. These are the main defence against a misread offset,
//! since no hardware is available to check against.

use es9_protocol::config::{Config, EqBand, Options};
use es9_protocol::decode::{self, DecodeError, Incoming};
use es9_protocol::encode;
use es9_protocol::frame::{self, FrameError};
use es9_protocol::links::Links;
use es9_protocol::tables::Family;

/// A configuration with a distinct value in every field, so a shifted offset shows up.
fn distinctive() -> Config {
    let mut c = Config {
        hpf: 0x55,
        options: Options {
            mixer2: true,
            midi_thru: true,
        },
        links: Links(0xDEAD_BEEF),
        midi_usb: 9,
        midi_din: 14,
        smoothing: 0xA5A5,
        ..Config::default()
    };
    for block in 0..4 {
        for ch in 0..8 {
            c.capture[block][ch] = (block * 8 + ch) as u8;
            c.outputs[block][ch] = (0x10 + block * 8 + ch) as u8;
        }
    }
    for mix in 0..16 {
        for ch in 0..8 {
            c.raw_mix[mix][ch] = (mix * 8 + ch) as u16 * 37;
        }
    }
    for (i, w) in c.macro_words.iter_mut().enumerate() {
        *w = (i as u32 * 13) & 0xFFFF;
    }
    for (i, d) in c.dc_offsets.iter_mut().enumerate() {
        *d = (i as i16 - 4) * 1000;
    }
    for block in 0..2 {
        for ch in 0..8 {
            for band in 0..4 {
                c.eq[block][ch][band] = EqBand {
                    enabled: (block + ch + band) % 2 == 0,
                    kind: ((ch + band) % 8) as u8,
                    freq: (ch * 4 + band) as u32 * 101,
                    q: (ch * 4 + band) as u32 * 57,
                    gain: (band as i32 - 2) * 900,
                };
            }
        }
    }
    for (i, r) in c.reserved.iter_mut().enumerate() {
        *r = (i as u32 * 7) & 0xFFFF;
    }
    c
}

#[test]
fn config_dump_round_trips() {
    let original = distinctive();
    let bytes = encode::config_dump(&original);
    // 5 header + 1 command + 2 zero bytes + 768 words + F7
    assert_eq!(bytes.len(), 5 + 1 + 2 + 768 * 3 + 1);
    match decode::decode(&bytes).expect("decodes") {
        Incoming::Config(decoded) => assert_eq!(*decoded, original),
        other => panic!("expected a config dump, got {other:?}"),
    }
}

#[test]
fn upload_chunks_reassemble_into_the_same_dump() {
    let original = distinctive();
    let chunks = encode::config_upload(&original);
    assert_eq!(chunks.len(), encode::UPLOAD_CHUNKS);

    let mut words = Vec::new();
    for (i, chunk) in chunks.iter().enumerate() {
        let msg = frame::parse(chunk).expect("valid frame");
        assert_eq!(msg.cmd, 0x09);
        assert_eq!(msg.payload[0], i as u8, "chunk index");
        assert_eq!(msg.payload[1], 0);
        words.extend(msg.payload[2..].chunks_exact(3).map(frame::join21));
    }
    assert_eq!(words, encode::config_words(&original));
}

#[test]
fn firmware_1_2_dump_is_rejected_not_misparsed() {
    // The version field is 3 in both formats, so only the command byte distinguishes
    // them. A 1.2 dump must fail loudly rather than decode to nonsense.
    let mut bytes = encode::config_dump(&distinctive());
    bytes[5] = decode::CMD_CONFIG_DUMP_V12;
    assert_eq!(decode::decode(&bytes), Err(DecodeError::FirmwareTooOld));
}

#[test]
fn unsupported_layout_version_is_reported() {
    let mut c = distinctive();
    c.version = 4;
    let bytes = encode::config_dump(&c);
    assert_eq!(
        decode::decode(&bytes),
        Err(DecodeError::UnsupportedVersion { found: 4 })
    );
}

#[test]
fn truncated_dump_is_rejected() {
    let bytes = encode::config_dump(&distinctive());
    let short = &bytes[..bytes.len() - 30];
    let mut truncated = short.to_vec();
    truncated.push(frame::EOX);
    assert!(matches!(
        decode::decode(&truncated),
        Err(DecodeError::ShortPayload { .. })
    ));
}

#[test]
fn message_strips_the_trailing_nul() {
    let mut v = frame::begin(0x32);
    v.extend_from_slice(b"OK\0");
    let bytes = frame::end(v);
    assert_eq!(
        decode::decode(&bytes),
        Ok(Incoming::Message("OK".to_string()))
    );
}

#[test]
fn sample_rate_is_scaled_by_four() {
    let mut v = frame::begin(0x14);
    frame::push21(&mut v, 96_000 / 4);
    assert_eq!(
        decode::decode(&frame::end(v)),
        Ok(Incoming::SampleRate(96_000))
    );
}

#[test]
fn raw_mix_change_decodes_mix_and_channel_from_the_command_byte() {
    let bytes = encode::set_raw_mix(11, 5, 0x1234);
    assert_eq!(
        decode::decode(&bytes),
        Ok(Incoming::RawMixChanged {
            mix: 11,
            channel: 5,
            value: 0x1234,
        })
    );
}

#[test]
fn mix_dump_splits_raw_and_macro_sections() {
    let mut v = frame::begin(0x11);
    for i in 0..128u32 {
        frame::push21(&mut v, i * 100);
    }
    for i in 0..128u8 {
        v.push(i);
        v.push(127 - i);
    }
    match decode::decode(&frame::end(v)).expect("decodes") {
        Incoming::Mix(m) => {
            assert_eq!(m.raw[0][0], 0);
            assert_eq!(m.raw[15][7], (127 * 100) as u16);
            assert_eq!(m.macro_cells[5].value, 5);
            assert_eq!(m.macro_cells[5].aux, 122);
        }
        other => panic!("expected a mix dump, got {other:?}"),
    }
}

#[test]
fn frame_rejects_foreign_and_malformed_messages() {
    assert_eq!(frame::parse(&[0xF0, 0xF7]), Err(FrameError::TooShort));
    assert_eq!(
        frame::parse(&[0xF0, 0x00, 0x21, 0x28, 0x19, 0x22, 0xF7]),
        Err(FrameError::BadHeader)
    );
    assert_eq!(
        frame::parse(&[0xF0, 0x00, 0x21, 0x27, 0x19, 0x22, 0x00]),
        Err(FrameError::MissingEox)
    );
    assert_eq!(
        frame::parse(&[0xF0, 0x00, 0x21, 0x27, 0x19, 0x22, 0x80, 0xF7]),
        Err(FrameError::NonDataByte { index: 6 })
    );
}

#[test]
fn word_packing_survives_the_full_range() {
    for v in [0u32, 1, 0x7F, 0x80, 0x3FFF, 0x4000, 0xFFFF, 0x1F_FFFF] {
        assert_eq!(frame::join21(&frame::split21(v)), v, "value {v:#X}");
    }
}

#[test]
fn signed_fields_survive_a_round_trip() {
    for v in [0i32, 1, -1, 32767, -32768, -1000] {
        assert_eq!(frame::sign_extend16(frame::to_unsigned16(v)), v);
    }
}

#[test]
fn link_bits_match_the_documented_layout() {
    // Derived from the config dump parser's bit assignments.
    assert_eq!(Links::bit(Family::Input, 0), Some(0));
    assert_eq!(Links::bit(Family::Input, 12), Some(6));
    assert_eq!(Links::bit(Family::Input, 14), None, "only 14 inputs exist");
    assert_eq!(Links::bit(Family::Bus, 0), Some(8));
    assert_eq!(Links::bit(Family::Bus, 14), Some(15));
    assert_eq!(Links::bit(Family::UsbLow, 0), Some(16));
    assert_eq!(Links::bit(Family::UsbHigh, 0), Some(20));
    assert_eq!(Links::bit(Family::Mix, 0), Some(24));
    assert_eq!(Links::bit(Family::SpdifOrMix2, 6), Some(31));
    assert_eq!(
        Links::bit(Family::Mix, 1),
        None,
        "pairs start on even indices"
    );
}
