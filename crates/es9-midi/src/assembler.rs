//! Reassembling SysEx messages from a MIDI input stream.
//!
//! A configuration dump is 2313 bytes, which is far larger than a normal MIDI message.
//! Backends differ in whether they deliver such a message whole or in fragments, so this
//! buffers until the terminating `F7` rather than trusting one callback to carry it all.
//!
//! Pure and synchronous, so it can be tested without a MIDI device.

/// Accumulates bytes into complete SysEx messages.
#[derive(Debug, Clone, Default)]
pub struct SysexAssembler {
    buf: Vec<u8>,
    in_message: bool,
}

impl SysexAssembler {
    /// Creates an empty assembler.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds bytes in, returning any messages completed by them.
    ///
    /// Realtime bytes may be interleaved anywhere in a MIDI stream, including inside a
    /// SysEx message, so they are discarded rather than corrupting the buffer. A new
    /// `F0` abandons an unterminated message, which is what a device reset looks like.
    pub fn push(&mut self, data: &[u8]) -> Vec<Vec<u8>> {
        let mut complete = Vec::new();
        for &b in data {
            match b {
                0xF8..=0xFF => continue, // realtime, may interrupt anything
                0xF0 => {
                    self.buf.clear();
                    self.buf.push(b);
                    self.in_message = true;
                }
                0xF7 if self.in_message => {
                    self.buf.push(b);
                    complete.push(core::mem::take(&mut self.buf));
                    self.in_message = false;
                }
                _ if self.in_message => self.buf.push(b),
                _ => {} // channel-voice traffic outside a SysEx message
            }
        }
        complete
    }

    /// Discards any partially received message.
    pub fn reset(&mut self) {
        self.buf.clear();
        self.in_message = false;
    }

    /// Bytes buffered in an incomplete message.
    pub fn pending(&self) -> usize {
        self.buf.len()
    }
}
