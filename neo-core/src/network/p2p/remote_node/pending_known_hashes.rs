//! Tracks recently announced hashes to avoid duplicate inventory processing.
use indexmap::IndexMap;
use std::time::{Duration, Instant};

use crate::UInt256;

#[derive(Clone, Copy)]
struct PendingKnownHash {
    timestamp: Instant,
}

/// Tracks hashes announced by peers to avoid re-requesting/duplicating inventories.
pub(crate) struct PendingKnownHashes {
    inner: IndexMap<UInt256, PendingKnownHash>,
    capacity: usize,
}

/// Maximum TTL for pending hashes (60 seconds).
/// Entries older than this will be pruned on next try_add operation.
const MAX_PENDING_TTL: Duration = Duration::from_secs(60);

impl PendingKnownHashes {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            inner: IndexMap::with_capacity(capacity),
            capacity,
        }
    }

    pub(crate) fn contains(&self, hash: &UInt256) -> bool {
        self.inner.contains_key(hash)
    }

    /// Attempts to add a hash to the pending set.
    /// Automatically prunes stale entries and enforces capacity limits.
    pub(crate) fn try_add(&mut self, hash: UInt256, timestamp: Instant) -> bool {
        // Auto-prune entries older than MAX_PENDING_TTL
        if let Some(cutoff) = timestamp.checked_sub(MAX_PENDING_TTL) {
            self.prune_older_than(cutoff);
        }

        // If at capacity after pruning, remove oldest entry to make room
        if self.inner.len() >= self.capacity && !self.inner.is_empty() {
            self.inner.shift_remove_index(0);
        }

        if self.inner.contains_key(&hash) {
            return false;
        }

        self.inner.insert(hash, PendingKnownHash { timestamp });
        true
    }

    pub(crate) fn remove(&mut self, hash: &UInt256) -> bool {
        self.inner.shift_remove(hash).is_some()
    }

    pub(crate) fn clear(&mut self) {
        let capacity = self.inner.capacity();
        self.inner = IndexMap::with_capacity(capacity);
    }

    pub(crate) fn prune_older_than(&mut self, cutoff: Instant) -> usize {
        let mut removed = 0;
        while let Some((_, entry)) = self.inner.first() {
            if entry.timestamp >= cutoff {
                break;
            }
            if self.inner.shift_remove_index(0).is_none() {
                break;
            }
            removed += 1;
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::PendingKnownHashes;
    use crate::UInt256;
    use std::time::{Duration, Instant};

    fn make_hash(byte: u8) -> UInt256 {
        let mut data = [0u8; 32];
        data[0] = byte;
        UInt256::from(data)
    }

    #[test]
    fn prune_older_than_removes_stale_front_entries() {
        let now = Instant::now();
        let mut cache = PendingKnownHashes::new(4);

        cache.try_add(make_hash(1), now - Duration::from_secs(55));
        cache.try_add(make_hash(2), now - Duration::from_secs(5));

        let removed = cache.prune_older_than(now - Duration::from_secs(30));
        assert_eq!(removed, 1);
        assert!(!cache.contains(&make_hash(1)));
        assert!(cache.contains(&make_hash(2)));
    }

    #[test]
    fn try_add_evicts_oldest_when_full() {
        let now = Instant::now();
        let mut cache = PendingKnownHashes::new(2);

        assert!(cache.try_add(make_hash(1), now));
        assert!(cache.try_add(make_hash(2), now));
        assert!(cache.try_add(make_hash(3), now));

        assert!(!cache.contains(&make_hash(1)));
        assert!(cache.contains(&make_hash(2)));
        assert!(cache.contains(&make_hash(3)));
    }

    #[test]
    fn duplicate_add_does_not_refresh_timestamp_when_not_full() {
        let now = Instant::now();
        let mut cache = PendingKnownHashes::new(4);
        let hash = make_hash(1);

        assert!(cache.try_add(hash, now));
        assert!(!cache.try_add(hash, now + Duration::from_secs(1)));

        assert_eq!(cache.inner.get(&hash).unwrap().timestamp, now);
    }

    #[test]
    fn full_duplicate_preserves_legacy_eviction_before_duplicate_check() {
        let now = Instant::now();
        let mut cache = PendingKnownHashes::new(2);

        assert!(cache.try_add(make_hash(1), now));
        assert!(cache.try_add(make_hash(2), now));
        assert!(!cache.try_add(make_hash(2), now + Duration::from_secs(1)));

        assert!(!cache.contains(&make_hash(1)));
        assert!(cache.contains(&make_hash(2)));
        assert_eq!(cache.inner.len(), 1);
    }

    #[test]
    fn zero_capacity_matches_legacy_single_entry_behavior() {
        let now = Instant::now();
        let mut cache = PendingKnownHashes::new(0);

        assert!(cache.try_add(make_hash(1), now));
        assert!(cache.try_add(make_hash(2), now));

        assert!(!cache.contains(&make_hash(1)));
        assert!(cache.contains(&make_hash(2)));
        assert_eq!(cache.inner.len(), 1);
    }

    #[test]
    fn clear_removes_all_hashes() {
        let now = Instant::now();
        let mut cache = PendingKnownHashes::new(2);

        cache.try_add(make_hash(1), now);
        cache.try_add(make_hash(2), now);
        cache.clear();

        assert!(!cache.contains(&make_hash(1)));
        assert!(!cache.contains(&make_hash(2)));
        assert_eq!(cache.inner.len(), 0);
    }
}
