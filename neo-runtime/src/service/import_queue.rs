//! # neo-runtime::service::import_queue
//!
//! Bounded block-import preverification queue.
//!
//! ## Boundary
//!
//! This module belongs to `neo-runtime`. It owns the shared queue contract and
//! concurrency policy for `BlockImport::check`, but it does not execute blocks,
//! persist state, compute state roots, or choose a sync/download strategy.
//!
//! ## Contents
//!
//! - `ImportQueue`: shared queue capability for sync and networking callers.
//! - `BlockImportQueue`: bounded concurrent preverification wrapper over a
//!   canonical [`BlockImport`].
//!
//! The queue mirrors the Polkadot SDK split between concurrent preverification
//! and ordered import. It runs [`BlockImport::check`] for a batch with bounded
//! concurrency, then hands the verified blocks to [`BlockImport::import_many`]
//! in their original order so state transition and durable persistence remain
//! single-path and deterministic.
//!
//! ## Status
//!
//! This is a reusable primitive that is **not yet instantiated by the
//! production sync path**. `BlockImportQueue` is constructed only under
//! `tests/`; the live import path calls [`BlockImport`] directly via
//! `BlockchainHandle::import_many` (driven by neo-blockchain's
//! `handle_block_inventory`). Full staged-sync wiring that would route through
//! this queue is deferred (see the sync roadmap / ADRs).

use crate::{
    BlockBatchImportOutcome, BlockImport, BlockOrigin, Service, ServiceError, ServiceResult,
};
use async_trait::async_trait;
use neo_payloads::Block;
use std::fmt;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

/// Import-queue capability shared by sync and networking callers.
#[async_trait]
pub trait ImportQueue: Service {
    /// Preverify `blocks` and submit the verified batch to the canonical import
    /// path.
    async fn push_blocks(
        &self,
        blocks: Vec<Block>,
        origin: BlockOrigin,
    ) -> ServiceResult<BlockBatchImportOutcome>;
}

/// Bounded concurrent preverification wrapper around a [`BlockImport`].
pub struct BlockImportQueue<I: BlockImport + ?Sized> {
    importer: Arc<I>,
    max_concurrency: usize,
}

impl<I: BlockImport + ?Sized> BlockImportQueue<I> {
    /// Creates a queue over `importer`.
    ///
    /// `max_concurrency == 0` is treated as `1` so callers cannot accidentally
    /// create a permanently stalled queue.
    #[must_use]
    pub fn new(importer: Arc<I>, max_concurrency: usize) -> Self {
        Self {
            importer,
            max_concurrency: max_concurrency.max(1),
        }
    }

    /// Returns the configured maximum number of concurrent preflight checks.
    #[must_use]
    pub const fn max_concurrency(&self) -> usize {
        self.max_concurrency
    }

    async fn preverify_blocks(&self, blocks: Vec<Block>) -> ServiceResult<Vec<Block>> {
        if blocks.is_empty() {
            return Ok(Vec::new());
        }

        let block_count = blocks.len();
        let semaphore = Arc::new(Semaphore::new(self.max_concurrency));
        let mut tasks = JoinSet::new();

        for (position, block) in blocks.into_iter().enumerate() {
            let importer = Arc::clone(&self.importer);
            let semaphore = Arc::clone(&semaphore);
            tasks.spawn(async move {
                let _permit = semaphore.acquire_owned().await.map_err(|err| {
                    ServiceError::unavailable(format!("import queue semaphore closed: {err}"))
                })?;
                importer.check(&block).await?;
                Ok::<_, ServiceError>((position, block))
            });
        }

        let mut verified = Vec::with_capacity(block_count);
        verified.resize_with(block_count, || None);

        while let Some(result) = tasks.join_next().await {
            match result {
                Ok(Ok((position, block))) => {
                    verified[position] = Some(block);
                }
                Ok(Err(error)) => {
                    tasks.abort_all();
                    return Err(error);
                }
                Err(error) => {
                    tasks.abort_all();
                    return Err(ServiceError::internal(format!(
                        "import queue preverification task failed: {error}"
                    )));
                }
            }
        }

        verified
            .into_iter()
            .enumerate()
            .map(|(position, block)| {
                block.ok_or_else(|| {
                    ServiceError::internal(format!(
                        "import queue preverification task {position} did not return a block"
                    ))
                })
            })
            .collect()
    }
}

impl<I: BlockImport + ?Sized> fmt::Debug for BlockImportQueue<I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BlockImportQueue")
            .field("importer", &self.importer.name())
            .field("max_concurrency", &self.max_concurrency)
            .finish()
    }
}

impl<I: BlockImport + ?Sized> Service for BlockImportQueue<I> {
    fn name(&self) -> &str {
        "BlockImportQueue"
    }
}

#[async_trait]
impl<I: BlockImport + ?Sized> ImportQueue for BlockImportQueue<I> {
    async fn push_blocks(
        &self,
        blocks: Vec<Block>,
        origin: BlockOrigin,
    ) -> ServiceResult<BlockBatchImportOutcome> {
        let verified = self.preverify_blocks(blocks).await?;
        if verified.is_empty() {
            return Ok(BlockBatchImportOutcome::new(0));
        }
        self.importer.import_many(verified, origin).await
    }
}

#[cfg(test)]
#[path = "../tests/service/import_queue.rs"]
mod tests;
