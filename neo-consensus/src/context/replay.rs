//! Bounded message-hash replay protection for consensus payloads.
//!
//! The cache is deliberately scoped to a consensus block round: view changes keep
//! entries so duplicate payloads cannot be replayed within the round, while a
//! new block clears the cache to bound memory over time.

use lru::LruCache;
use neo_primitives::UInt256;
use std::num::NonZeroUsize;

use super::{ConsensusContext, MAX_MESSAGE_CACHE_SIZE};

impl ConsensusContext {
    pub(super) fn new_seen_message_cache() -> LruCache<UInt256, ()> {
        let capacity = match NonZeroUsize::new(MAX_MESSAGE_CACHE_SIZE) {
            Some(capacity) => capacity,
            None => NonZeroUsize::MIN,
        };
        LruCache::new(capacity)
    }

    /// Checks if a message hash has been seen before (replay attack prevention)
    ///
    /// This method is critical for preventing replay attacks where an attacker
    /// could retransmit valid consensus messages to disrupt the protocol.
    ///
    /// # Arguments
    /// * `hash` - The message hash to check
    ///
    /// # Returns
    /// * `true` if the message has been seen before
    /// * `false` if this is a new message
    #[must_use]
    pub fn has_seen_message(&self, hash: &UInt256) -> bool {
        self.seen_message_hashes.contains(hash)
    }

    /// Marks a message hash as seen (replay attack prevention)
    ///
    /// This method adds the message hash to the cache to prevent duplicate processing.
    /// The cache is automatically cleared when starting a new block via `reset_for_new_block()`.
    ///
    /// Security: uses a bounded LRU cache (`MAX_MESSAGE_CACHE_SIZE`) to prevent memory
    /// exhaustion attacks while avoiding a clear-all window for recently seen messages.
    ///
    /// # Arguments
    /// * `hash` - The message hash to mark as seen
    pub fn mark_message_seen(&mut self, hash: &UInt256) {
        if self.seen_message_hashes.contains(hash) {
            return;
        }
        self.seen_message_hashes.put(*hash, ());
    }
}
