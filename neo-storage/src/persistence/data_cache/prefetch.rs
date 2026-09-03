use crate::types::StorageKey;

/// Prefetch pattern detection for sequential access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefetchPattern {
    /// No prefetching.
    None,
    /// Sequential forward access (ascending keys).
    SequentialForward,
    /// Sequential backward access (descending keys).
    SequentialBackward,
    /// Strided access (fixed offset between keys).
    Strided,
}

/// Tracks access patterns for intelligent prefetching.
pub(super) struct AccessPatternTracker {
    /// Last accessed key (for pattern detection)
    last_key: Option<StorageKey>,
    /// Last access sequence number
    last_seq: u64,
    /// Detected pattern
    pattern: PrefetchPattern,
    /// Confidence score (0-100)
    confidence: u8,
    /// Sequential access counter
    sequential_count: u32,
}

impl AccessPatternTracker {
    pub(super) fn new() -> Self {
        Self {
            last_key: None,
            last_seq: 0,
            pattern: PrefetchPattern::None,
            confidence: 0,
            sequential_count: 0,
        }
    }

    /// Record an access and update pattern detection.
    pub(super) fn record_access(&mut self, key: &StorageKey, seq: u64) -> PrefetchPattern {
        if let Some(ref last) = self.last_key {
            let key_bytes = key.as_bytes();
            let last_bytes = last.as_bytes();

            // Check for sequential access patterns
            if *key_bytes > *last_bytes {
                // Potential forward sequential
                if self.pattern == PrefetchPattern::SequentialForward {
                    self.sequential_count += 1;
                    self.confidence = (self.confidence + 10).min(100);
                } else {
                    self.pattern = PrefetchPattern::SequentialForward;
                    self.sequential_count = 1;
                    self.confidence = 20;
                }
            } else if key_bytes < last_bytes {
                // Potential backward sequential
                if self.pattern == PrefetchPattern::SequentialBackward {
                    self.sequential_count += 1;
                    self.confidence = (self.confidence + 10).min(100);
                } else {
                    self.pattern = PrefetchPattern::SequentialBackward;
                    self.sequential_count = 1;
                    self.confidence = 20;
                }
            } else {
                // No pattern or reset
                self.confidence = self.confidence.saturating_sub(5);
                if self.confidence < 10 {
                    self.pattern = PrefetchPattern::None;
                    self.sequential_count = 0;
                }
            }
        }

        self.last_key = Some(key.clone());
        self.last_seq = seq;
        self.pattern
    }

    /// Get the current detected pattern if confidence is high enough.
    pub(super) fn current_pattern(&self, threshold: u8) -> PrefetchPattern {
        if self.confidence >= threshold {
            self.pattern
        } else {
            PrefetchPattern::None
        }
    }

    #[allow(dead_code)]
    pub(super) fn reset(&mut self) {
        *self = Self::new();
    }
}
