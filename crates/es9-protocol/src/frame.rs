//! SysEx framing, and the 7-bit packing the ES-9 uses for wide values.
//!
//! Every message is `F0 00 21 27 19 <cmd> [payload] F7`, where `00 21 27` is the
//! Expert Sleepers manufacturer ID and `19` identifies the ES-9.

/// Manufacturer ID and device ID that prefix every ES-9 message.
pub const HEADER: [u8; 5] = [0xF0, 0x00, 0x21, 0x27, 0x19];

/// SysEx end-of-exclusive byte.
pub const EOX: u8 = 0xF7;

/// A parsed message: the command byte and the payload between it and `F7`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message<'a> {
    /// The command byte following the header.
    pub cmd: u8,
    /// Payload bytes, excluding the trailing `F7`.
    pub payload: &'a [u8],
}

/// Why a byte slice could not be read as an ES-9 message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    /// Fewer bytes than the shortest legal message.
    TooShort,
    /// The leading five bytes were not the ES-9 header.
    BadHeader,
    /// The final byte was not `F7`.
    MissingEox,
    /// A payload byte had bit 7 set, which SysEx forbids.
    NonDataByte {
        /// Index into the original slice.
        index: usize,
    },
}

impl core::fmt::Display for FrameError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::TooShort => write!(f, "message is shorter than the 7-byte minimum"),
            Self::BadHeader => write!(f, "not an ES-9 message (header mismatch)"),
            Self::MissingEox => write!(f, "message does not end with F7"),
            Self::NonDataByte { index } => {
                write!(
                    f,
                    "byte {index} has bit 7 set; SysEx payloads must be 7-bit"
                )
            }
        }
    }
}

impl core::error::Error for FrameError {}

/// Splits a slice into command and payload, validating the frame.
///
/// Payload bytes are checked for the 7-bit constraint, because a device or driver
/// that corrupts a stream tends to do so by dropping or duplicating bytes, and an
/// out-of-range byte is the cheapest place to notice.
pub fn parse(data: &[u8]) -> Result<Message<'_>, FrameError> {
    if data.len() < HEADER.len() + 2 {
        return Err(FrameError::TooShort);
    }
    if data[..HEADER.len()] != HEADER {
        return Err(FrameError::BadHeader);
    }
    if *data.last().expect("length checked above") != EOX {
        return Err(FrameError::MissingEox);
    }
    let payload = &data[HEADER.len() + 1..data.len() - 1];
    for (i, b) in payload.iter().enumerate() {
        if b & 0x80 != 0 {
            return Err(FrameError::NonDataByte {
                index: HEADER.len() + 1 + i,
            });
        }
    }
    Ok(Message {
        cmd: data[HEADER.len()],
        payload,
    })
}

/// Starts a message buffer with the header and a command byte.
pub fn begin(cmd: u8) -> Vec<u8> {
    let mut v = Vec::with_capacity(16);
    v.extend_from_slice(&HEADER);
    v.push(cmd);
    v
}

/// Appends `F7`, completing a message.
pub fn end(mut v: Vec<u8>) -> Vec<u8> {
    v.push(EOX);
    v
}

/// Splits a value into three 7-bit bytes, most significant first.
///
/// The ES-9 calls these 21-bit words, though no field uses more than 16 bits of range.
pub fn split21(v: u32) -> [u8; 3] {
    [
        ((v >> 14) & 0x7F) as u8,
        ((v >> 7) & 0x7F) as u8,
        (v & 0x7F) as u8,
    ]
}

/// Reassembles a 21-bit word from three 7-bit bytes.
///
/// # Panics
/// Panics if `b` is shorter than three bytes.
pub fn join21(b: &[u8]) -> u32 {
    assert!(b.len() >= 3, "join21 needs three bytes");
    (u32::from(b[0]) << 14) | (u32::from(b[1]) << 7) | u32::from(b[2])
}

/// Reassembles a 14-bit value from two 7-bit bytes, as used by the usage report.
///
/// # Panics
/// Panics if `b` is shorter than two bytes.
pub fn join14(b: &[u8]) -> u32 {
    assert!(b.len() >= 2, "join14 needs two bytes");
    (u32::from(b[0]) << 7) | u32::from(b[1])
}

/// Sign-extends a 16-bit field carried in a 21-bit word.
///
/// Output DC offsets and EQ gains are signed but transmitted unsigned.
pub fn sign_extend16(v: u32) -> i32 {
    ((v << 16) as i32) >> 16
}

/// Truncates a signed value into the 16-bit two's-complement form the module expects.
pub fn to_unsigned16(v: i32) -> u32 {
    (v as u32) & 0xFFFF
}

/// Appends a 21-bit word.
pub fn push21(v: &mut Vec<u8>, value: u32) {
    v.extend_from_slice(&split21(value));
}
