//! Shared block-import contract for sync, consensus, and RPC submission.
//!
//! `neo-blockchain` owns the concrete command loop and persistence pipeline.
//! This module owns the narrow domain trait that higher layers should depend
//! on when they only need to submit blocks to the canonical import path.

use async_trait::async_trait;
use neo_payloads::Block;
use neo_primitives::UInt256;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::task::JoinSet;

use crate::error::ServiceError;
use crate::errors::ServiceResult;
use crate::services::Service;

/// Where a block import request came from.
///
/// The origin is deliberately semantic rather than transport-specific. The
/// concrete blockchain service may use it for metrics, policy, or validation
/// modes, while callers avoid depending on `BlockchainCommand` internals.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockOrigin {
    /// A peer-relayed or sync-downloaded block.
    Sync,
    /// A block proposed or committed by the local consensus engine.
    Consensus,
    /// A user or tool submitted the block through RPC/CLI.
    Rpc,
    /// Local trusted bulk import, such as a validated `chain.acc` package.
    TrustedLocal,
}

/// Canonical tip after a successful block import.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportedTip {
    /// Hash of the imported block.
    pub hash: UInt256,
    /// Height of the imported block.
    pub height: u32,
    /// Block timestamp in milliseconds since Unix epoch.
    pub timestamp: u64,
}

impl ImportedTip {
    /// Build the imported-tip summary from a block.
    pub fn from_block(block: &Block) -> Result<Self, ServiceError> {
        let hash = block.try_hash().map_err(|error| {
            ServiceError::invalid_input(format!("block hash serialization failed: {error}"))
        })?;
        Ok(Self {
            hash,
            height: block.index(),
            timestamp: block.timestamp(),
        })
    }
}

/// Result of submitting one block to the canonical import path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockImportOutcome {
    /// The block advanced the canonical tip.
    Imported(ImportedTip),
    /// The service accepted the request but did not advance the tip.
    ///
    /// This includes duplicate blocks, parked out-of-order blocks, or blocks
    /// rejected by validation after the request reached the canonical service
    /// path. Future implementations can split this outcome into narrower
    /// variants without changing the trait call shape.
    NotImported {
        /// Hash of the submitted block.
        hash: UInt256,
        /// Height declared by the submitted block.
        height: u32,
    },
}

/// Result of submitting a consecutive block batch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockBatchImportOutcome {
    /// Number of input blocks processed by the canonical import path.
    pub processed: usize,
}

impl BlockBatchImportOutcome {
    /// Construct a batch-import outcome from a processed count.
    #[must_use]
    pub fn new(processed: usize) -> Self {
        Self { processed }
    }
}

/// Canonical block-import API shared by consensus and sync.
///
/// Implementations must route every successful import through the same
/// validation, execution, native-persist, state-root, and durable-store path.
/// This keeps consensus, P2P sync, fast-sync replay, and RPC submission from
/// growing separate block acceptance rules.
#[async_trait]
pub trait BlockImport: Service {
    /// Cheap preflight for a block before committing to the full import path.
    async fn check(&self, block: &Block) -> Result<(), ServiceError>;

    /// Submit one block to the canonical import path.
    async fn import(
        &self,
        block: Block,
        origin: BlockOrigin,
    ) -> Result<BlockImportOutcome, ServiceError>;

    /// Submit a consecutive batch to the canonical import path.
    async fn import_many(
        &self,
        blocks: Vec<Block>,
        origin: BlockOrigin,
    ) -> Result<BlockBatchImportOutcome, ServiceError> {
        let mut processed = 0;
        for block in blocks {
            match self.import(block, origin).await? {
                BlockImportOutcome::Imported(_) | BlockImportOutcome::NotImported { .. } => {
                    processed += 1;
                }
            }
        }
        Ok(BlockBatchImportOutcome::new(processed))
    }
}

/// Queue boundary for verified, ordered block import.
///
/// Implementations may perform bounded stateless or cheap stateful
/// preverification before forwarding to the canonical [`BlockImport`] chain.
/// They must preserve the input block order when they call the import path and
/// must skip import entirely if preflight fails.
#[async_trait]
pub trait ImportQueue: Service {
    /// Push a batch of candidate blocks toward the canonical import path.
    async fn push_blocks(
        &self,
        blocks: Vec<Block>,
        origin: BlockOrigin,
    ) -> ServiceResult<BlockBatchImportOutcome>;
}

/// Bounded preverification queue in front of the canonical block-import path.
///
/// The queue mirrors Substrate's `ImportQueue` boundary without owning Neo's
/// block execution or storage logic. It is responsible only for cheap
/// concurrent [`BlockImport::check`] calls and then hands the original
/// canonical-order batch to [`BlockImport::import_many`].
#[derive(Debug)]
pub struct BlockImportQueue<I: BlockImport + ?Sized> {
    importer: Arc<I>,
    max_concurrency: usize,
}

impl<I> BlockImportQueue<I>
where
    I: BlockImport + ?Sized,
{
    /// Create an import queue over `importer`.
    ///
    /// `max_concurrency` is clamped to at least one so callers can wire config
    /// values directly without creating a permanently stalled queue.
    #[must_use]
    pub fn new(importer: Arc<I>, max_concurrency: usize) -> Self {
        Self {
            importer,
            max_concurrency: max_concurrency.max(1),
        }
    }

    /// Returns the bounded number of blocks checked concurrently.
    #[must_use]
    pub const fn max_concurrency(&self) -> usize {
        self.max_concurrency
    }

    /// Returns the importer behind this queue.
    #[must_use]
    pub fn importer(&self) -> &Arc<I> {
        &self.importer
    }

    /// Validate every block, then import the original ordered batch.
    ///
    /// This inherent method is the ergonomic entry point for callers that own a
    /// concrete `BlockImportQueue`. Generic sync code should depend on the
    /// [`ImportQueue`] trait instead.
    pub async fn push_blocks(
        &self,
        blocks: Vec<Block>,
        origin: BlockOrigin,
    ) -> ServiceResult<BlockBatchImportOutcome> {
        self.preverify_and_import(blocks, origin).await
    }

    async fn preverify_and_import(
        &self,
        blocks: Vec<Block>,
        origin: BlockOrigin,
    ) -> ServiceResult<BlockBatchImportOutcome> {
        let block_count = blocks.len();
        if block_count == 0 {
            return Ok(BlockBatchImportOutcome::new(0));
        }

        let mut pending_blocks = blocks.into_iter().enumerate();
        let mut check_tasks = JoinSet::new();
        let mut checked_blocks = (0..block_count).map(|_| None).collect::<Vec<_>>();

        while check_tasks.len() < self.max_concurrency {
            let Some((position, block)) = pending_blocks.next() else {
                break;
            };
            let importer = Arc::clone(&self.importer);
            check_tasks.spawn(async move {
                let result = importer.check(&block).await;
                (position, block, result)
            });
        }

        while let Some(joined) = check_tasks.join_next().await {
            let (position, block, check_result) = joined.map_err(|error| {
                ServiceError::internal(format!("block import check task failed: {error}"))
            })?;
            if let Err(error) = check_result {
                check_tasks.abort_all();
                return Err(error);
            }
            checked_blocks[position] = Some(block);

            if let Some((position, block)) = pending_blocks.next() {
                let importer = Arc::clone(&self.importer);
                check_tasks.spawn(async move {
                    let result = importer.check(&block).await;
                    (position, block, result)
                });
            }
        }

        let blocks = checked_blocks
            .into_iter()
            .map(|block| {
                block.ok_or_else(|| {
                    ServiceError::internal("block import queue lost a checked block")
                })
            })
            .collect::<ServiceResult<Vec<_>>>()?;

        self.importer.import_many(blocks, origin).await
    }
}

impl<I> Service for BlockImportQueue<I>
where
    I: BlockImport + ?Sized,
{
    fn name(&self) -> &str {
        "BlockImportQueue"
    }
}

#[async_trait]
impl<I> ImportQueue for BlockImportQueue<I>
where
    I: BlockImport + ?Sized,
{
    /// Validate every block, then import the original ordered batch.
    ///
    /// A check failure skips import entirely. This keeps preverification and
    /// canonical persistence separated: out-of-order downloader work can be
    /// parallel, while state mutation still occurs through one deterministic
    /// ordered path.
    async fn push_blocks(
        &self,
        blocks: Vec<Block>,
        origin: BlockOrigin,
    ) -> ServiceResult<BlockBatchImportOutcome> {
        self.preverify_and_import(blocks, origin).await
    }
}

#[cfg(test)]
#[path = "../tests/service/block_import.rs"]
mod tests;
