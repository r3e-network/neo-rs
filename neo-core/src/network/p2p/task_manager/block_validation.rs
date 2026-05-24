use crate::network::p2p::payloads::block::Block;
use crate::{CoreError, UInt256};

#[derive(Debug, Clone)]
pub(super) enum IncomingBlockOutcome {
    Store {
        index: u32,
        block: Block,
    },
    KeepExisting,
    Disconnect {
        index: u32,
        reason: IncomingBlockDisconnect,
        hash_error: Option<CoreError>,
        remove_cached: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum IncomingBlockDisconnect {
    InvalidIncomingHash,
    ConflictingBlock,
    InvalidCachedHash,
}

impl IncomingBlockDisconnect {
    pub(super) fn reason(self) -> &'static str {
        match self {
            Self::InvalidIncomingHash => "invalid block hash",
            Self::ConflictingBlock => "conflicting block received",
            Self::InvalidCachedHash => "invalid cached block hash",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PersistedBlockMatch {
    Matches,
    Mismatch,
    Unhashable(CoreError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum BlockHashMatch {
    Matches,
    DoesNotMatch,
    Unhashable(CoreError),
}

pub(super) fn validate_incoming_block(
    mut block: Block,
    existing: Option<&Block>,
) -> IncomingBlockOutcome {
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

pub(super) fn persisted_block_hash(block: &Block) -> Result<UInt256, CoreError> {
    let mut block = block.clone();
    block.try_hash()
}

pub(super) fn match_persisted_block(
    mut stored: Block,
    persisted_hash: &UInt256,
) -> PersistedBlockMatch {
    match stored.try_hash() {
        Ok(stored_hash) if stored_hash == *persisted_hash => PersistedBlockMatch::Matches,
        Ok(_) => PersistedBlockMatch::Mismatch,
        Err(error) => PersistedBlockMatch::Unhashable(error),
    }
}

pub(super) fn block_matches_hash(block: &Block, expected_hash: &UInt256) -> BlockHashMatch {
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

        assert!(matches!(
            outcome,
            IncomingBlockOutcome::Store { index: 7, .. }
        ));
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
}
