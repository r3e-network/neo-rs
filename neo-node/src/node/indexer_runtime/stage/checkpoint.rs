//! # neo-node::node::indexer_runtime::stage::checkpoint
//!
//! Canonical checkpoint verification, ahead pruning, and invalid-prefix reset.
//!
//! ## Boundary
//!
//! Recovery compares the projection with committed canonical blocks and may
//! mutate only the optional indexer store. Canonical Ledger state is read-only.
//!
//! ## Contents
//!
//! - `reconcile_to_target`: prune a projection ahead of the canonical tip.
//! - `resume_height_from_checkpoint`: resume a valid prefix or durably reset an
//!   invalid one before rebuild batches are published.

use neo_indexer::{IndexerStatus, NotificationIndexRecord};
use neo_payloads::Block;
use tracing::warn;

use super::{CanonicalIndexSource, IndexStage, IndexStageError};

impl<P, N> IndexStage<P, N>
where
    P: CanonicalIndexSource,
    N: Fn(&Block) -> Vec<NotificationIndexRecord> + Send + Sync,
{
    pub(super) fn reconcile_to_target(&self, target_height: u32) -> Result<(), IndexStageError> {
        let removed = self
            .indexer
            .revert_to_height(target_height)
            .map_err(|source| IndexStageError::Reconcile {
                height: target_height,
                source,
            })?;
        if !removed.is_empty() {
            self.indexer
                .flush_durable()
                .map_err(|source| IndexStageError::Durability {
                    start: target_height,
                    end: target_height,
                    source,
                })?;
        }
        Ok(())
    }

    pub(super) async fn resume_height_from_checkpoint(
        &self,
        target_height: u32,
        checkpoint: IndexerStatus,
    ) -> Result<Option<u32>, IndexStageError> {
        let Some(indexed_height) = checkpoint.indexed_height else {
            return Ok(Some(0));
        };
        let expected_blocks = u64::from(indexed_height).saturating_add(1);
        let block_count_is_contiguous =
            u64::try_from(checkpoint.indexed_blocks).unwrap_or(u64::MAX) == expected_blocks;
        let checkpoint_matches = if indexed_height <= target_height
            && block_count_is_contiguous
            && checkpoint.indexed_hash.is_some()
        {
            let block = self
                .source
                .block_by_height(indexed_height)
                .await
                .map_err(|source| IndexStageError::BlockRead {
                    height: indexed_height,
                    source,
                })?;
            match block {
                Some(block) => {
                    let hash = block
                        .try_hash()
                        .map_err(|error| IndexStageError::BlockHash {
                            height: indexed_height,
                            reason: error.to_string(),
                        })?;
                    checkpoint.indexed_hash == Some(hash)
                }
                None => false,
            }
        } else {
            false
        };

        if checkpoint_matches {
            return Ok(indexed_height.checked_add(1));
        }

        warn!(
            target: "neo::indexer",
            target_height,
            indexed_height = ?checkpoint.indexed_height,
            indexed_hash = ?checkpoint.indexed_hash,
            indexed_blocks = checkpoint.indexed_blocks,
            "invalid Index-stage checkpoint; clearing the projection before canonical rebuild"
        );
        self.reset_invalid_checkpoint()?;
        Ok(Some(0))
    }

    pub(super) fn reset_invalid_checkpoint(&self) -> Result<(), IndexStageError> {
        self.indexer
            .clear()
            .map_err(|source| IndexStageError::CheckpointReset { source })?;
        self.indexer
            .flush_durable()
            .map_err(|source| IndexStageError::CheckpointResetDurability { source })
    }
}
