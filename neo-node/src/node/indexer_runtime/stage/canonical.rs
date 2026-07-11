//! # neo-node::node::indexer_runtime::stage::canonical
//!
//! Hash-linked canonical batch reads and fixed-target revalidation.
//!
//! ## Boundary
//!
//! These helpers prove that blocks read during one stage run form a coherent
//! extension of its verified checkpoint. They read canonical data but mutate
//! only the optional indexer when a moved target invalidates the projection.
//!
//! ## Contents
//!
//! - `read_batch`: validate requested heights and parent hashes while reading.
//! - `verify_canonical_target`: reject and reset a projection whose fixed target
//!   changed while batches were being committed.

use neo_indexer::NotificationIndexRecord;
use neo_payloads::Block;
use neo_primitives::UInt256;

use super::{CanonicalIndexSource, IndexStage, IndexStageError};

pub(super) struct PendingIndexBlock {
    pub(super) block: Block,
    pub(super) notifications: Vec<NotificationIndexRecord>,
}

impl<P, N> IndexStage<P, N>
where
    P: CanonicalIndexSource,
    N: Fn(&Block) -> Vec<NotificationIndexRecord> + Send + Sync,
{
    pub(super) async fn read_batch(
        &self,
        start: u32,
        end: u32,
        mut expected_parent_hash: UInt256,
    ) -> Result<(Vec<PendingIndexBlock>, UInt256), IndexStageError> {
        let mut pending = Vec::with_capacity(
            usize::try_from(u64::from(end).saturating_sub(u64::from(start)) + 1)
                .unwrap_or(usize::MAX),
        );
        for height in start..=end {
            let block = self
                .source
                .block_by_height(height)
                .await
                .map_err(|source| IndexStageError::BlockRead { height, source })?
                .ok_or(IndexStageError::MissingCanonicalBlock { height })?;
            if block.index() != height {
                return Err(IndexStageError::CanonicalHeightMismatch {
                    requested: height,
                    actual: block.index(),
                });
            }
            let actual_parent = *block.prev_hash();
            if actual_parent != expected_parent_hash {
                return Err(IndexStageError::ParentHashMismatch {
                    height,
                    expected: expected_parent_hash,
                    actual: actual_parent,
                });
            }
            expected_parent_hash =
                block
                    .try_hash()
                    .map_err(|error| IndexStageError::BlockHash {
                        height,
                        reason: error.to_string(),
                    })?;
            let notifications = (self.notifications)(&block);
            pending.push(PendingIndexBlock {
                block,
                notifications,
            });
        }
        Ok((pending, expected_parent_hash))
    }

    pub(super) async fn verify_canonical_target(
        &self,
        target_height: u32,
        indexed_hash: Option<UInt256>,
    ) -> Result<(), IndexStageError> {
        let canonical = self
            .source
            .block_by_height(target_height)
            .await
            .map_err(|source| IndexStageError::BlockRead {
                height: target_height,
                source,
            })?;
        let canonical_hash = match canonical {
            Some(block) => {
                if block.index() != target_height {
                    return Err(IndexStageError::CanonicalHeightMismatch {
                        requested: target_height,
                        actual: block.index(),
                    });
                }
                Some(
                    block
                        .try_hash()
                        .map_err(|error| IndexStageError::BlockHash {
                            height: target_height,
                            reason: error.to_string(),
                        })?,
                )
            }
            None => None,
        };
        if canonical_hash == indexed_hash {
            return Ok(());
        }

        self.reset_invalid_checkpoint()?;
        Err(IndexStageError::CanonicalTargetMoved {
            height: target_height,
            indexed: indexed_hash,
            canonical: canonical_hash,
        })
    }
}
