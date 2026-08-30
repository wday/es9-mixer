//! Coalescing for continuous controls.
//!
//! A fader drag produces a value on every pointer event. The reference tool sends SysEx
//! for each one, which floods a MIDI link that also carries the module's own traffic.
//! This keeps the latest value per control and releases it on a fixed interval, so a drag
//! costs one message per tick rather than one per pixel.
//!
//! Time is supplied by the caller rather than read from a clock, so the behaviour is
//! deterministic under test.

use std::collections::BTreeMap;

/// Coalesces rapid updates, keeping only the most recent value per key.
#[derive(Debug, Clone)]
pub struct Coalescer<K: Ord, V> {
    pending: BTreeMap<K, V>,
    interval_ms: u64,
    last_flush_ms: u64,
}

impl<K: Ord, V> Coalescer<K, V> {
    /// Creates a coalescer that releases values at most every `interval_ms`.
    pub fn new(interval_ms: u64) -> Self {
        Self {
            pending: BTreeMap::new(),
            interval_ms,
            last_flush_ms: 0,
        }
    }

    /// Records the latest value for a control, replacing any pending one.
    pub fn push(&mut self, key: K, value: V) {
        self.pending.insert(key, value);
    }

    /// Whether anything is waiting to be sent.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Releases pending values if the interval has elapsed.
    ///
    /// Returns an empty vector when it is too soon, so a caller can poll freely.
    pub fn flush(&mut self, now_ms: u64) -> Vec<(K, V)> {
        if self.pending.is_empty() || now_ms.saturating_sub(self.last_flush_ms) < self.interval_ms {
            return Vec::new();
        }
        self.last_flush_ms = now_ms;
        core::mem::take(&mut self.pending).into_iter().collect()
    }

    /// Releases everything immediately, regardless of the interval.
    ///
    /// Use when a drag ends, so the final value is never left sitting in the buffer.
    pub fn flush_now(&mut self, now_ms: u64) -> Vec<(K, V)> {
        self.last_flush_ms = now_ms;
        core::mem::take(&mut self.pending).into_iter().collect()
    }
}
