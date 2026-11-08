//! Incoming-block validation and hash-matching helpers for the task manager.
//!
//! Mirrors `neo-core::network::p2p::task_manager::block_validation`. These are
//! pure functions over the canonical protocol [`Block`]: they decide whether a
//! peer-delivered block should be stored, deduplicated against a cached copy, or
//! cause the peer to be disconnected, and whether a persisted block matches the
//! hash the task manager expected.
//!
//! The `neo-core` original carries a `CoreError` for unhashable headers; here the
//! hash failure type is `neo_io::IoError`, the error `Block::try_hash` returns.

use neo_io::IoError;
use neo_primitives::UInt256;
use neo_protocol::payloads::Block;

/// Outcome of validating a peer-delivered block against any cached copy.
///
/// `Store` carries the whole block while the other variants are cheap outcomes;
/// the enum is constructed once per incoming block, so boxing the payload would
/// add allocation/indirection for no benefit. Lint consciously suppressed.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum IncomingBlockOutcome {
    /// No conflicting cached copy: store the block for validation/persistence.
    Store {
        /// Block height.
        index: u32,
        /// The block to store.
        block: Block,
    },
    /// A cached copy with the same hash already exists; keep it.
    KeepExisting,
    /// The block must be rejected and the peer disconnected.
    Disconnect {
        /// Block height.
        index: u32,
        /// Why the peer is being disconnected.
        reason: IncomingBlockDisconnect,
        /// The hash computation error, when the disconnect was caused by one.
        hash_error: Option<IoError>,
        /// Whether a cached copy at this height must also be removed.
        remove_cached: bool,
    },
}

/// Reason an incoming block forces a peer disconnect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncomingBlockDisconnect {
    /// The incoming block's header could not be hashed.
    InvalidIncomingHash,
    /// The incoming block conflicts with a different cached block at the height.
    ConflictingBlock,
    /// The cached block's header could not be hashed.
    InvalidCachedHash,
}

impl IncomingBlockDisconnect {
    /// Human-readable disconnect reason.
    pub fn reason(self) -> &'static str {
        match self {
            Self::InvalidIncomingHash => "invalid block hash",
            Self::ConflictingBlock => "conflicting block received",
            Self::InvalidCachedHash => "invalid cached block hash",
        }
    }
}

/// Result of comparing a stored block against the persisted hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersistedBlockMatch {
    /// The stored block hashes to the persisted hash.
    Matches,
    /// The stored block hashes to a different value.
    Mismatch,
    /// The stored block could not be hashed.
    Unhashable(IoError),
}

/// Result of comparing a block against an expected hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockHashMatch {
    /// The block hashes to the expected value.
    Matches,
    /// The block hashes to a different value.
    DoesNotMatch,
    /// The block could not be hashed.
    Unhashable(IoError),
}

/// Validates an incoming block against an optional cached copy at the same height.
pub fn validate_incoming_block(mut block: Block, existing: Option<&Block>) -> IncomingBlockOutcome {
    let index = block.index();
    let incoming_hash = match block.try_hash() {
        Ok(hash) => hash,
        Err(error) => {
            return IncomingBlockOutcome::Disconnect {
                index,
                reason: IncomingBlockDisconnect::InvalidIncomingHash,
                hash_error: Some(error),
                remove_cached: false,
            };
        }
    };

    let Some(existing) = existing else {
        return IncomingBlockOutcome::Store { index, block };
    };

    // Clone required: `existing` is `&Block` (shared reference), but `try_hash`
    // requires `&mut self`.
    let mut existing = existing.clone();
    match existing.try_hash() {
        Ok(existing_hash) if existing_hash == incoming_hash => IncomingBlockOutcome::KeepExisting,
        Ok(_) => IncomingBlockOutcome::Disconnect {
            index,
            reason: IncomingBlockDisconnect::ConflictingBlock,
            hash_error: None,
            remove_cached: false,
        },
        Err(error) => IncomingBlockOutcome::Disconnect {
            index,
            reason: IncomingBlockDisconnect::InvalidCachedHash,
            hash_error: Some(error),
            remove_cached: true,
        },
    }
}

/// Computes the hash of a persisted block.
pub fn persisted_block_hash(block: &Block) -> Result<UInt256, IoError> {
    // Clone required: `try_hash` requires `&mut self` but callers hold `&Block`.
    let mut block = block.clone();
    block.try_hash()
}

/// Compares a stored block against the hash that was actually persisted.
pub fn match_persisted_block(mut stored: Block, persisted_hash: &UInt256) -> PersistedBlockMatch {
    match stored.try_hash() {
        Ok(stored_hash) if stored_hash == *persisted_hash => PersistedBlockMatch::Matches,
        Ok(_) => PersistedBlockMatch::Mismatch,
        Err(error) => PersistedBlockMatch::Unhashable(error),
    }
}

/// Compares a block against an expected hash.
pub fn block_matches_hash(block: &Block, expected_hash: &UInt256) -> BlockHashMatch {
    // Clone required: `try_hash` requires `&mut self` but callers hold `&Block`.
    let mut candidate = block.clone();
    match candidate.try_hash() {
        Ok(candidate_hash) if candidate_hash == *expected_hash => BlockHashMatch::Matches,
        Ok(_) => BlockHashMatch::DoesNotMatch,
        Err(error) => BlockHashMatch::Unhashable(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_with_index_and_nonce(index: u32, nonce: u64) -> Block {
        let mut block = Block::new();
        block.header.set_index(index);
        block.header.set_nonce(nonce);
        block
    }

    fn block_hash(block: &Block) -> UInt256 {
        let mut block = block.clone();
        block.try_hash().expect("block hash")
    }

    #[test]
    fn incoming_block_without_cached_copy_is_stored() {
        let block = block_with_index_and_nonce(7, 1);

        let outcome = validate_incoming_block(block, None);

        assert!(matches!(outcome, IncomingBlockOutcome::Store { index: 7, .. }));
    }

    #[test]
    fn incoming_block_matching_cached_copy_is_kept() {
        let block = block_with_index_and_nonce(7, 1);
        let cached = block.clone();

        let outcome = validate_incoming_block(block, Some(&cached));

        assert!(matches!(outcome, IncomingBlockOutcome::KeepExisting));
    }

    #[test]
    fn incoming_block_conflicting_with_cached_copy_disconnects() {
        let block = block_with_index_and_nonce(7, 1);
        let cached = block_with_index_and_nonce(7, 2);

        let outcome = validate_incoming_block(block, Some(&cached));

        assert!(matches!(
            outcome,
            IncomingBlockOutcome::Disconnect {
                index: 7,
                reason: IncomingBlockDisconnect::ConflictingBlock,
                hash_error: None,
                remove_cached: false
            }
        ));
    }

    #[test]
    fn persisted_block_match_compares_hashes() {
        let block = block_with_index_and_nonce(7, 1);
        let hash = block_hash(&block);
        let conflicting_hash = block_hash(&block_with_index_and_nonce(7, 2));

        assert_eq!(
            match_persisted_block(block.clone(), &hash),
            PersistedBlockMatch::Matches
        );
        assert_eq!(
            match_persisted_block(block, &conflicting_hash),
            PersistedBlockMatch::Mismatch
        );
    }

    #[test]
    fn block_hash_match_compares_hashes() {
        let block = block_with_index_and_nonce(7, 1);
        let hash = block_hash(&block);
        let other = block_hash(&block_with_index_and_nonce(8, 1));

        assert_eq!(block_matches_hash(&block, &hash), BlockHashMatch::Matches);
        assert_eq!(
            block_matches_hash(&block, &other),
            BlockHashMatch::DoesNotMatch
        );
    }

    #[test]
    fn disconnect_reasons_are_stable_strings() {
        assert_eq!(
            IncomingBlockDisconnect::InvalidIncomingHash.reason(),
            "invalid block hash"
        );
        assert_eq!(
            IncomingBlockDisconnect::ConflictingBlock.reason(),
            "conflicting block received"
        );
        assert_eq!(
            IncomingBlockDisconnect::InvalidCachedHash.reason(),
            "invalid cached block hash"
        );
    }
}
