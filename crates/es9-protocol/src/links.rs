//! The stereo link bitmap.
//!
//! Links are stored per signal family rather than per channel, and they do far more
//! than merge two faders in the UI: a link changes which MIDI CC numbers control what.
//! See [`crate::ccmap`].

use crate::tables::Family;

/// The 32-bit stereo link bitmap from the configuration.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Links(pub u32);

impl Links {
    /// Bit position for a pair, identified by family and the *even* channel index.
    ///
    /// Returns `None` if the index is odd or beyond the family's pair count.
    pub fn bit(family: Family, even_index: u8) -> Option<u32> {
        if !even_index.is_multiple_of(2) {
            return None;
        }
        let pair = u32::from(even_index) / 2;
        if pair >= family.pair_count() {
            return None;
        }
        Some(family.link_bit_base() + pair)
    }

    /// Whether the pair starting at `even_index` is linked as stereo.
    pub fn is_linked(self, family: Family, even_index: u8) -> bool {
        Self::bit(family, even_index).is_some_and(|b| self.0 & (1 << b) != 0)
    }

    /// Whether `index` is part of a stereo pair, whether it is the left or right half.
    pub fn in_stereo_pair(self, family: Family, index: u8) -> bool {
        self.is_linked(family, index & !1)
    }

    /// Sets or clears a link.
    pub fn set(&mut self, family: Family, even_index: u8, on: bool) {
        if let Some(b) = Self::bit(family, even_index) {
            if on {
                self.0 |= 1 << b;
            } else {
                self.0 &= !(1 << b);
            }
        }
    }

    /// The link index the `33H` message uses, which is the raw bit position.
    pub fn message_index(family: Family, even_index: u8) -> Option<u8> {
        Self::bit(family, even_index).map(|b| b as u8)
    }
}
