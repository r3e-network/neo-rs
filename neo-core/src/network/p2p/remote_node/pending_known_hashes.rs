//! Tracks recently announced hashes to avoid duplicate inventory processing.
use neo_io_crate::KeyedCollectionSlim;
use std::time::{Duration, Instant};

use crate::UInt256;

#[derive(Clone, Copy)]
pub(crate) struct PendingKnownHash {
    pub hash: UInt256,
    pub timestamp: Instant,
}

/// Tracks hashes announced by peers to avoid re-requesting/duplicating inventories.
pub(crate) struct PendingKnownHashes {
    inner: KeyedCollectionSlim<UInt256, PendingKnownHash>,
    capacity: usize,
}

/// Maximum TTL for pending hashes (60 seconds).
/// Entries older than this will be pruned on next try_add operation.
const MAX_PENDING_TTL: Duration = Duration::from_secs(60);

impl PendingKnownHashes {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            inner: KeyedCollectionSlim::with_selector(capacity, |entry: &PendingKnownHash| {
                entry.hash
            }),
            capacity,
        }
    }

    pub(crate) fn contains(&self, hash: &UInt256) -> bool {
        self.inner.contains(hash)
    }

    /// Attempts to add a hash to the pending set.
    /// Automatically prunes stale entries and enforces capacity limits.
    pub(crate) fn try_add(&mut self, hash: UInt256, timestamp: Instant) -> bool {
        // Auto-prune entries older than MAX_PENDING_TTL
        if let Some(cutoff) = timestamp.checked_sub(MAX_PENDING_TTL) {
            self.prune_older_than(cutoff);
        }

        // If at capacity after pruning, remove oldest entry to make room
        if self.inner.count() >= self.capacity {
            self.inner.remove_first();
        }

        self.inner.try_add(PendingKnownHash { hash, timestamp })
    }

    pub(crate) fn remove(&mut self, hash: &UInt256) -> bool {
        self.inner.remove(hash)
    }

    pub(crate) fn clear(&mut self) {
        self.inner.clear();
    }

    pub(crate) fn prune_older_than(&mut self, cutoff: Instant) -> usize {
        let mut removed = 0;
        loop {
            let Some(entry) = self.inner.first_or_default() else {
                break;
            };
            if entry.timestamp >= cutoff {
                break;
            }
            if !self.inner.remove_first() {
                break;
            }
            removed += 1;
        }
        removed
    }
}
