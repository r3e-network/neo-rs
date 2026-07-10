//! Blockchain handle import boundary.
//!
//! This module adapts the public handle to `neo_runtime::BlockImport` and to
//! explicit user/package import requests. Live peer inventory remains in
//! `inventory`; this path waits for ordered service-loop replies and reports a
//! typed import outcome to RPC, chain package loading, and runtime sync drivers.

use std::sync::Arc;

use neo_payloads::Block;
use neo_runtime::{
    BlockBatchImportOutcome, BlockImport, BlockImportOutcome, BlockOrigin, ImportedTip,
    ServiceError,
};

use super::BlockchainHandle;
use crate::command::{BlockchainCommand, ImportBlocksReply};
use crate::import::Import;

impl BlockchainHandle {
    /// Import an externally supplied block and return the typed import outcome.
    ///
    /// `Imported` means verification/persistence advanced the canonical tip.
    /// `NotImported` means the service rejected the block or parked it without
    /// changing the tip. Live peer/consensus inventory should still use the
    /// `submit_inventory_*` methods because those preserve relay, parking, and
    /// mempool-maintenance semantics.
    pub async fn import_block(&self, block: Block) -> Result<BlockImportOutcome, ServiceError> {
        let tip = ImportedTip::from_block(&block)?;
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.cmd_tx
            .send(BlockchainCommand::ImportBlock {
                block: Arc::new(block),
                reply: reply_tx,
            })
            .await
            .map_err(|_| {
                ServiceError::ServiceUnavailable("blockchain command channel closed".to_string())
            })?;
        let imported = reply_rx.await.map_err(|_| {
            ServiceError::ServiceUnavailable("blockchain command reply dropped".to_string())
        })?;
        if imported {
            Ok(BlockImportOutcome::Imported(tip))
        } else {
            Ok(BlockImportOutcome::NotImported {
                hash: tip.hash,
                height: tip.height,
            })
        }
    }

    /// Import a consecutive batch of blocks and wait until the service has
    /// processed it. Resolves with the number of supplied blocks accepted as
    /// part of the consecutive prefix before the first gap or rejected block
    /// stops the import loop. Already-persisted prefix blocks count as
    /// processed so `chain.acc` dumps that include genesis do not look
    /// truncated to the caller.
    pub async fn import_blocks(
        &self,
        blocks: Vec<Block>,
        verify: bool,
    ) -> Result<usize, ServiceError> {
        self.import_blocks_with_mode(blocks, verify, false).await
    }

    /// Import a trusted bulk-sync batch and skip replay-only artifacts that
    /// cold-sync consumers intentionally do not read.
    pub async fn import_blocks_bulk(
        &self,
        blocks: Vec<Block>,
        verify: bool,
    ) -> Result<usize, ServiceError> {
        self.import_blocks_with_mode(blocks, verify, true).await
    }

    /// Import a trusted bulk-sync batch and return the detailed service-side
    /// timing/composition reply.
    pub async fn import_blocks_bulk_detailed(
        &self,
        blocks: Vec<Block>,
        verify: bool,
    ) -> Result<ImportBlocksReply, ServiceError> {
        self.import_blocks_reply_with_mode(blocks, verify, true)
            .await
    }

    async fn import_blocks_with_mode(
        &self,
        blocks: Vec<Block>,
        verify: bool,
        bulk_sync: bool,
    ) -> Result<usize, ServiceError> {
        let reply = self
            .import_blocks_reply_with_mode(blocks, verify, bulk_sync)
            .await?;
        if let Some(error) = reply.error {
            return Err(ServiceError::InvalidState(format!(
                "block import finalization failed after importing {} blocks: {error}",
                reply.imported
            )));
        }
        Ok(reply.imported)
    }

    async fn import_blocks_reply_with_mode(
        &self,
        blocks: Vec<Block>,
        verify: bool,
        bulk_sync: bool,
    ) -> Result<ImportBlocksReply, ServiceError> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.cmd_tx
            .send(BlockchainCommand::ImportBlocks {
                import: Import {
                    blocks,
                    verify,
                    bulk_sync,
                },
                reply: reply_tx,
            })
            .await
            .map_err(|_| {
                ServiceError::ServiceUnavailable("blockchain command channel closed".to_string())
            })?;
        let reply = reply_rx.await.map_err(|_| {
            ServiceError::ServiceUnavailable("blockchain command reply dropped".to_string())
        })?;
        Ok(reply)
    }
}

impl BlockImport for BlockchainHandle {
    fn check(
        &self,
        block: &Block,
    ) -> impl std::future::Future<Output = Result<(), ServiceError>> + Send {
        std::future::ready((|| {
            block.try_hash().map_err(|error| {
                ServiceError::invalid_input(format!("block hash serialization failed: {error}"))
            })?;
            crate::block_validation::BlockValidator::validate_import_integrity(block)
                .map_err(|error| ServiceError::invalid_input(error.to_string()))?;
            Ok(())
        })())
    }

    fn import(
        &self,
        block: Block,
        _origin: BlockOrigin,
    ) -> impl std::future::Future<Output = Result<BlockImportOutcome, ServiceError>> + Send {
        self.import_block(block)
    }

    async fn import_many(
        &self,
        blocks: Vec<Block>,
        origin: BlockOrigin,
    ) -> Result<BlockBatchImportOutcome, ServiceError> {
        let verify = !matches!(origin, BlockOrigin::TrustedLocal);
        let processed = if matches!(origin, BlockOrigin::TrustedLocal) {
            self.import_blocks_bulk(blocks, verify).await?
        } else {
            self.import_blocks(blocks, verify).await?
        };
        Ok(BlockBatchImportOutcome::new(processed))
    }
}
