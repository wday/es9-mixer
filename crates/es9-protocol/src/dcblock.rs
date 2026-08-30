//! Input DC-blocking filters.
//!
//! The fourteen analogue inputs are DC coupled so they can carry CV. Each input ADC has
//! an optional DC-blocking high-pass filter: on for audio, **off for CV**, since the
//! filter would strip exactly the constant offset a CV signal consists of.
//!
//! The filters switch **in pairs**, not individually, and the pairs are not the obvious
//! ones — inputs 3 and 8 share a filter. That is not arbitrary: a pair is two adjacent
//! ADC channels, and the mapping from input number to ADC channel is the same
//! permutation as [`crate::tables::INPUT_CAPTURE_LOOKUP`]. `tests/dcblock.rs` asserts
//! that correspondence rather than trusting the table below on its own.
//!
//! Presenting these as seven per-input switches would be a lie, and an expensive one:
//! turning the filter on for input 8 to clean up an audio signal also turns it on for
//! input 3, which silently destroys a CV patched there.

/// Number of DC-blocking filters, one per input pair.
pub const PAIR_COUNT: usize = 7;

/// The input pair each bit of the bitmap controls, as 1-based input numbers.
///
/// Bit index is the position in this array. Taken from the official configuration tool
/// and corroborated against the input permutation.
pub const PAIRS: [(u8, u8); PAIR_COUNT] =
    [(1, 2), (3, 8), (4, 5), (6, 7), (9, 10), (11, 12), (13, 14)];

/// The inputs controlled by one bit, or `None` if the bit is out of range.
pub fn pair(bit: u8) -> Option<(u8, u8)> {
    PAIRS.get(usize::from(bit)).copied()
}

/// The bit controlling a given 1-based input number.
pub fn bit_for_input(input: u8) -> Option<u8> {
    PAIRS
        .iter()
        .position(|&(a, b)| a == input || b == input)
        .map(|i| i as u8)
}

/// Whether the filter for a pair is engaged.
pub fn is_on(bitmap: u8, bit: u8) -> bool {
    bit < PAIR_COUNT as u8 && bitmap & (1 << bit) != 0
}

/// Returns the bitmap with one pair's filter set on or off.
///
/// An out-of-range bit leaves the bitmap untouched: the payload field is seven bits, and
/// writing an eighth would be a value the module never sends back.
pub fn with(bitmap: u8, bit: u8, on: bool) -> u8 {
    if bit >= PAIR_COUNT as u8 {
        return bitmap;
    }
    if on {
        bitmap | (1 << bit)
    } else {
        bitmap & !(1 << bit)
    }
}

/// A label for one pair, for example `"Inputs 3/8"`.
pub fn label(bit: u8) -> Option<String> {
    pair(bit).map(|(a, b)| format!("Inputs {a}/{b}"))
}
