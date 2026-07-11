//! # neo-node::node::indexer_runtime::stage
//!
//! Bounded, crash-resumable Index-stage follower over committed canonical
//! blocks.
//!
//! ## Boundary
//!
//! This module projects canonical blocks after `Import` has completed. It must
//! not repeat validation, execution, native persistence, or state-root work.
//!
//! ## Contents
//!
//! - `CanonicalIndexSource`: committed block capabilities required by the stage.
//! - `canonical`: block-link validation and fixed-target revalidation.
//! - `checkpoint`: canonical checkpoint reconciliation and durable reset.
//! - `IndexStage`: bounded canonical batch execution.
//! - `IndexStageOutcome` / `IndexStageError`: typed operational results.

use std::sync::Arc;

use neo_indexer::{IndexBlockBatchEntry, IndexerService, IndexerStatus, NotificationIndexRecord};
use neo_payloads::Block;
use neo_primitives::UInt256;
use neo_runtime::{ServiceError, ServiceResult, SyncStageKind};

mod canonical;
mod checkpoint;

pub(super) const DEFAULT_INDEX_STAGE_BATCH_SIZE: u32 = 256;

/// Canonical block capabilities consumed by the Index stage.
pub(crate) trait CanonicalIndexSource: Clone + Send + Sync + 'static {
    /// Return the canonical height that this stage run should target.
    fn chain_height(&self) -> impl Future<Output = ServiceResult<u32>> + Send;

    /// Return one committed canonical block by height.
    fn block_by_height(
        &self,
        height: u32,
    ) -> impl Future<Output = ServiceResult<Option<Block>>> + Send;
}

impl CanonicalIndexSource for neo_blockchain::BlockchainHandle {
    fn chain_height(&self) -> impl Future<Output = ServiceResult<u32>> + Send {
        let blockchain = self.clone();
        async move { blockchain.get_height().await }
    }

    fn block_by_height(
        &self,
        height: u32,
    ) -> impl Future<Output = ServiceResult<Option<Block>>> + Send {
        let blockchain = self.clone();
        async move { blockchain.get_block_by_height(height).await }
    }
}

/// Completed work and authoritative durable checkpoint for one Index run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IndexStageOutcome {
    pub(crate) stage: SyncStageKind,
    pub(crate) start_height: Option<u32>,
    pub(crate) target_height: u32,
    pub(crate) processed_blocks: u64,
    pub(crate) committed_batches: u64,
    pub(crate) checkpoint: IndexerStatus,
}

/// Failures that prevent the Index stage from advancing its contiguous prefix.
#[derive(Debug, thiserror::Error)]
pub(crate) enum IndexStageError {
    #[error("failed to read canonical chain height: {source}")]
    ChainHeight {
        #[source]
        source: ServiceError,
    },
    #[error("failed to read canonical block {height}: {source}")]
    BlockRead {
        height: u32,
        #[source]
        source: ServiceError,
    },
    #[error("canonical block {height} is missing during Index stage")]
    MissingCanonicalBlock { height: u32 },
    #[error("canonical source returned block height {actual} for requested height {requested}")]
    CanonicalHeightMismatch { requested: u32, actual: u32 },
    #[error(
        "canonical block {height} does not extend the indexed prefix: expected parent {expected}, got {actual}"
    )]
    ParentHashMismatch {
        height: u32,
        expected: UInt256,
        actual: UInt256,
    },
    #[error("failed to hash canonical block {height}: {reason}")]
    BlockHash { height: u32, reason: String },
    #[error(
        "canonical target {height} moved during Index stage: indexed {indexed:?}, canonical {canonical:?}"
    )]
    CanonicalTargetMoved {
        height: u32,
        indexed: Option<UInt256>,
        canonical: Option<UInt256>,
    },
    #[error("failed to reconcile indexer to canonical height {height}: {source}")]
    Reconcile {
        height: u32,
        #[source]
        source: neo_indexer::IndexerError,
    },
    #[error("failed to clear an invalid indexer checkpoint: {source}")]
    CheckpointReset {
        #[source]
        source: neo_indexer::IndexerError,
    },
    #[error("failed to fence the cleared indexer checkpoint: {source}")]
    CheckpointResetDurability {
        #[source]
        source: neo_indexer::IndexerError,
    },
    #[error("failed to index canonical batch [{start}, {end}]: {source}")]
    Batch {
        start: u32,
        end: u32,
        #[source]
        source: neo_indexer::IndexerError,
    },
    #[error("failed to fence Index stage batch [{start}, {end}]: {source}")]
    Durability {
        start: u32,
        end: u32,
        #[source]
        source: neo_indexer::IndexerError,
    },
    #[error("failed to fence Index stage checkpoint at target {target}: {source}")]
    CheckpointDurability {
        target: u32,
        #[source]
        source: neo_indexer::IndexerError,
    },
    #[error(
        "Index stage finished at non-contiguous checkpoint: target {target}, indexed height {indexed_height:?}, indexed blocks {indexed_blocks}"
    )]
    NonContiguousCheckpoint {
        target: u32,
        indexed_height: Option<u32>,
        indexed_blocks: usize,
    },
}

/// Bounded, crash-resumable Index stage.
pub(crate) struct IndexStage<P, N>
where
    P: CanonicalIndexSource,
    N: Fn(&Block) -> Vec<NotificationIndexRecord> + Send + Sync,
{
    source: P,
    indexer: Arc<IndexerService>,
    notifications: N,
    batch_size: u32,
}

impl<P, N> IndexStage<P, N>
where
    P: CanonicalIndexSource,
    N: Fn(&Block) -> Vec<NotificationIndexRecord> + Send + Sync,
{
    /// Compose an Index stage from its canonical source and projection sink.
    pub(crate) fn new(source: P, indexer: Arc<IndexerService>, notifications: N) -> Self {
        Self {
            source,
            indexer,
            notifications,
            batch_size: DEFAULT_INDEX_STAGE_BATCH_SIZE,
        }
    }

    /// Override the bounded persistence batch size in stage tests.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn with_batch_size(mut self, batch_size: u32) -> Self {
        self.batch_size = batch_size.max(1);
        self
    }

    /// Advance the durable indexer checkpoint to a fixed canonical tip.
    pub(crate) async fn execute_to_tip(&self) -> Result<IndexStageOutcome, IndexStageError> {
        let target_height = self
            .source
            .chain_height()
            .await
            .map_err(|source| IndexStageError::ChainHeight { source })?;
        let mut checkpoint = self.indexer.projection_checkpoint();
        if checkpoint
            .indexed_height
            .is_some_and(|indexed_height| indexed_height > target_height)
        {
            self.reconcile_to_target(target_height)?;
            checkpoint = self.indexer.projection_checkpoint();
        }
        let start_height = self
            .resume_height_from_checkpoint(target_height, checkpoint)
            .await?;

        let mut next_height = start_height;
        let mut expected_parent_hash = if start_height == Some(0) {
            UInt256::zero()
        } else {
            checkpoint.indexed_hash.unwrap_or_default()
        };
        let mut processed_blocks = 0u64;
        let mut committed_batches = 0u64;
        while let Some(start) = next_height {
            if start > target_height {
                break;
            }
            let end = target_height.min(start.saturating_add(self.batch_size.saturating_sub(1)));
            let (mut pending, batch_tip_hash) =
                self.read_batch(start, end, expected_parent_hash).await?;
            expected_parent_hash = batch_tip_hash;
            let entries = pending
                .iter_mut()
                .map(|entry| {
                    let notifications = std::mem::take(&mut entry.notifications);
                    if notifications.is_empty() {
                        IndexBlockBatchEntry::block_only(&entry.block)
                    } else {
                        IndexBlockBatchEntry::with_notifications(&entry.block, notifications)
                    }
                })
                .collect::<Vec<_>>();
            self.indexer
                .index_block_batch(entries)
                .map_err(|source| IndexStageError::Batch { start, end, source })?;
            self.indexer
                .flush_durable()
                .map_err(|source| IndexStageError::Durability { start, end, source })?;

            processed_blocks = processed_blocks
                .saturating_add(u64::from(end).saturating_sub(u64::from(start)) + 1);
            committed_batches = committed_batches.saturating_add(1);
            if end == target_height {
                break;
            }
            next_height = end.checked_add(1);
            tokio::task::yield_now().await;
        }

        // A prior run may have committed its last batch but failed its durable
        // media fence. Retry that pending fence even when this run has no new
        // blocks, before reporting the checkpoint as durable.
        self.indexer
            .flush_durable()
            .map_err(|source| IndexStageError::CheckpointDurability {
                target: target_height,
                source,
            })?;

        let checkpoint = self.indexer.projection_checkpoint();
        let expected_blocks = u64::from(target_height).saturating_add(1);
        if !checkpoint.is_synced_with(Some(target_height))
            || u64::try_from(checkpoint.indexed_blocks).unwrap_or(u64::MAX) != expected_blocks
        {
            return Err(IndexStageError::NonContiguousCheckpoint {
                target: target_height,
                indexed_height: checkpoint.indexed_height,
                indexed_blocks: checkpoint.indexed_blocks,
            });
        }
        self.verify_canonical_target(target_height, checkpoint.indexed_hash)
            .await?;

        Ok(IndexStageOutcome {
            stage: SyncStageKind::Index,
            start_height,
            target_height,
            processed_blocks,
            committed_batches,
            checkpoint,
        })
    }
}
