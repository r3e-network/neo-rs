//! Service method handlers for [`crate::service::BlockchainService`].
//!
//! Each command variant has a corresponding `async fn` method on the
//! service. The dispatch loop in [`crate::service::BlockchainService::dispatch`]
//! just `match`es on the command enum and calls the right method. The service
//! stays explicit: no dynamic downcasting and no per-message trait machinery.
//!
//! The handlers own the service-side Neo protocol decisions: block/header
//! sequencing, native persistence, transaction admission, extensible payload
//! verification, and cache maintenance.

use std::sync::Arc;
use std::time::Instant;

use neo_error::{CoreError, CoreResult};
use neo_payloads::block::Block;
use tracing::{debug, warn};

use crate::command::{ImportBlocksReply, ImportBlocksStats};
use crate::empty_block_fast_forward::stage_empty_block_fast_forward;
use crate::import::Import;
use crate::internal::ImportDisposition;
use crate::native_persist::NativePersistOptions;
use crate::relay_result::RelayResult;
use crate::service::{BlockchainService, MempoolLike};
use crate::service_context::BlockPersistContext;

use super::consensus_witness_stage::{NeoConsensusWitnessStage, SnapshotConsensusWitnessContext};
use super::stage_traits::EngineError;
use super::verified_import_pipeline::VerifiedImportPipeline;

#[path = "../handlers/empty_fast_forward.rs"]
mod empty_fast_forward;
#[path = "../handlers/extensible.rs"]
mod extensible;
#[path = "../handlers/headers.rs"]
mod headers;
#[path = "../handlers/initialize.rs"]
mod initialize;
#[path = "../handlers/persist_completed.rs"]
mod persist_completed;
#[path = "../handlers/reverify.rs"]
mod reverify;
#[path = "../handlers/transactions.rs"]
mod transactions;

impl<S, M> BlockchainService<S, M>
where
    S: crate::service_context::SystemContext,
    M: MempoolLike,
{
    fn pipeline_error(block: &Block, error: EngineError) -> CoreError {
        match error {
            EngineError::ValidationFailed { reason, .. } => {
                CoreError::other(format!("block {}: {reason}", block.index()))
            }
            other => CoreError::other(format!("block {}: {other}", block.index())),
        }
    }

    fn verify_consensus_witness_against_store(&self, block: &Block) -> CoreResult<()> {
        let settings = self.system.settings();
        let snapshot = self.system.store_snapshot().ok_or_else(|| {
            CoreError::other(format!(
                "block {}: store snapshot unavailable",
                block.index()
            ))
        })?;
        self.verify_consensus_witness_against_snapshot_with_native_provider(
            block,
            settings,
            snapshot,
            self.system.native_contract_provider(),
        )
    }

    fn verify_consensus_witness_against_snapshot_with_native_provider(
        &self,
        block: &Block,
        settings: Arc<neo_config::ProtocolSettings>,
        snapshot: Arc<neo_storage::DataCache>,
        native_contract_provider: Option<
            Arc<dyn neo_execution::native_contract_provider::NativeContractProvider>,
        >,
    ) -> CoreResult<()> {
        let stage = NeoConsensusWitnessStage::new(Arc::new(SnapshotConsensusWitnessContext::new(
            settings,
            snapshot,
            native_contract_provider,
        )));
        stage
            .verify_block(block)
            .map_err(|error| Self::pipeline_error(block, error))
    }

    async fn verify_import_block_with_pipeline(
        &self,
        block: &Block,
        current_height: u32,
        bulk_sync: bool,
        settings: Arc<neo_config::ProtocolSettings>,
        snapshot: Arc<neo_storage::DataCache>,
        native_contract_provider: Option<
            Arc<dyn neo_execution::native_contract_provider::NativeContractProvider>,
        >,
    ) -> CoreResult<()> {
        VerifiedImportPipeline::verify_block(
            block,
            current_height,
            bulk_sync,
            settings,
            snapshot,
            native_contract_provider,
        )
        .await
        .map_err(|error| Self::pipeline_error(block, error))
    }

    fn ensure_block_matches_cached_header(
        &self,
        index: u32,
        hash: neo_primitives::UInt256,
    ) -> CoreResult<()> {
        if let Some(cached_header) = self.header_cache.get(index) {
            let cached_hash = cached_header.hash();
            if cached_hash != hash {
                return Err(CoreError::other(format!(
                    "block {index}: hash does not match cached header"
                )));
            }
        }
        Ok(())
    }

    /// Handle a [`BlockchainCommand::Import`] request.
    pub(crate) async fn handle_import(&self, import: Import) -> ImportBlocksReply {
        let mut imported = 0usize;
        let mut stats = ImportBlocksStats::default();
        let bulk_sync = import.bulk_sync;
        let persist_options = if bulk_sync {
            NativePersistOptions {
                capture_replay_artifacts: false,
            }
        } else {
            NativePersistOptions::default()
        };
        let persist_context = if bulk_sync {
            BlockPersistContext::bulk_sync()
        } else {
            BlockPersistContext::live()
        };
        let blocks = import.blocks;
        let mut batch_persist_resources = None;
        let mut batch_persist_resources_loaded = false;
        let mut last_imported_height = None;
        let mut position = 0usize;
        while position < blocks.len() {
            let block = &blocks[position];
            let index = block.index();
            let current_height = self.ledger.current_height();
            match ImportDisposition::classify_import_block(current_height, index) {
                ImportDisposition::AlreadySeen => {
                    imported += 1;
                    position += 1;
                    continue;
                }
                ImportDisposition::FutureGap => {
                    warn!(
                        target: "neo",
                        expected = current_height + 1,
                        actual = index,
                        "import block out of sequence"
                    );
                    return ImportBlocksReply::ok_with_stats(imported, stats);
                }
                ImportDisposition::NextExpected => {}
            }

            if bulk_sync && !batch_persist_resources_loaded {
                match self.batch_persist_resources(index) {
                    Ok(resources) => {
                        batch_persist_resources = resources;
                    }
                    Err(error) => {
                        warn!(
                            target: "neo",
                            %error,
                            height = index,
                            "import aborted: native persistence resource setup failed"
                        );
                        return ImportBlocksReply::ok_with_stats(imported, stats);
                    }
                }
                batch_persist_resources_loaded = true;
            }

            if bulk_sync
                && !import.verify
                && self.system.allows_empty_block_fast_forward()
                && let Some(resources) = &batch_persist_resources
            {
                let run = Self::collect_empty_fast_forward_run(
                    &blocks,
                    position,
                    current_height,
                    resources.settings.as_ref(),
                    &resources.native_persist,
                );
                if !run.is_empty() {
                    let empty_start = Instant::now();
                    match stage_empty_block_fast_forward(
                        Arc::clone(&resources.snapshot),
                        &run,
                        resources.settings.as_ref(),
                        persist_options,
                        persist_context,
                        &resources.native_persist,
                        current_height,
                    ) {
                        Ok(staged) => {
                            staged.commit();
                            stats.empty_blocks += run.len();
                            stats.empty_elapsed += empty_start.elapsed();
                            if let Some(last_block) = run.last() {
                                // Fast-forward is only enabled when no component
                                // needs the per-block observer stream. Keep
                                // ledger history in the durable store and only
                                // advance the hot in-memory tip for the batch.
                                self.ledger.record_tip(last_block.index());
                                imported += run.len();
                                last_imported_height = Some(last_block.index());
                            }
                            position += run.len();
                            continue;
                        }
                        Err(error) => {
                            debug!(
                                target: "neo::sync",
                                height = index,
                                error = %error,
                                "empty-block fast-forward fell back to normal persistence"
                            );
                        }
                    }
                }
            }

            if import.verify {
                let verify_result = if let Some(resources) = &batch_persist_resources {
                    self.verify_import_block_with_pipeline(
                        block,
                        current_height,
                        bulk_sync,
                        Arc::clone(&resources.settings),
                        Arc::clone(&resources.snapshot),
                        Some(resources.native_persist.provider()),
                    )
                    .await
                } else {
                    let snapshot = match self.system.store_snapshot() {
                        Some(snapshot) => snapshot,
                        None => {
                            warn!(
                                target: "neo",
                                height = index,
                                "import aborted: store snapshot unavailable for block validation"
                            );
                            return ImportBlocksReply::ok_with_stats(imported, stats);
                        }
                    };
                    self.verify_import_block_with_pipeline(
                        block,
                        current_height,
                        bulk_sync,
                        self.system.settings(),
                        snapshot,
                        self.system.native_contract_provider(),
                    )
                    .await
                };
                if let Err(error) = verify_result {
                    warn!(
                        target: "neo",
                        %error,
                        height = index,
                        "import aborted: block verification failed"
                    );
                    return ImportBlocksReply::ok_with_stats(imported, stats);
                }
            }

            if bulk_sync && let Some(resources) = &batch_persist_resources {
                let empty_start = Instant::now();
                match self.persist_empty_block_with_committing_fast_forward(
                    block,
                    current_height,
                    resources,
                    persist_options,
                    persist_context,
                ) {
                    Ok(true) => {
                        stats.empty_blocks += 1;
                        stats.empty_elapsed += empty_start.elapsed();
                        imported += 1;
                        last_imported_height = Some(index);
                        position += 1;
                        continue;
                    }
                    Ok(false) => {}
                    Err(error) => {
                        warn!(
                            target: "neo",
                            height = index,
                            %error,
                            "import aborted: empty-block committing fast-forward failed"
                        );
                        return ImportBlocksReply::ok_with_stats(imported, stats);
                    }
                }
            }

            // C# Blockchain.OnImport runs `Persist(block)` — the state
            // transition — before the block becomes the new tip.
            let transaction_block = !blocks[position].transactions.is_empty();
            let clone_start = transaction_block.then(Instant::now);
            let block = Arc::new(blocks[position].clone());
            if let Some(start) = clone_start {
                stats.transaction_block_clone_elapsed += start.elapsed();
            }
            let transaction_block = !block.transactions.is_empty();
            let transaction_start = transaction_block.then(Instant::now);
            let persisted = if bulk_sync {
                if let Some(resources) = &batch_persist_resources {
                    self.persist_block_sequence_with_resources(
                        Arc::clone(&block),
                        persist_options,
                        resources,
                    )
                } else {
                    self.persist_block_sequence_with_options(Arc::clone(&block), persist_options)
                        .await
                }
            } else {
                self.persist_block_sequence_with_options(Arc::clone(&block), persist_options)
                    .await
            };
            if !persisted {
                warn!(
                    target: "neo",
                    height = index,
                    "import aborted: native persistence pipeline failed"
                );
                return ImportBlocksReply::ok_with_stats(imported, stats);
            }
            if let Some(start) = transaction_start {
                stats.transaction_blocks += 1;
                stats.transaction_elapsed += start.elapsed();
            }

            let ledger_insert_start = transaction_block.then(Instant::now);
            if let Err(error) = self.ledger.insert_block_arc(Arc::clone(&block)) {
                warn!(
                    target: "neo",
                    %error,
                    height = index,
                    "failed to import block into ledger cache"
                );
                return ImportBlocksReply::ok_with_stats(imported, stats);
            }
            if let Some(start) = ledger_insert_start {
                stats.transaction_ledger_insert_elapsed += start.elapsed();
            }

            // Normal live imports flush each block immediately. Trusted
            // bulk-sync imports keep staging into the shared snapshot and flush
            // once after the accepted batch, avoiding one RocksDB commit per
            // block while preserving per-block native/state transitions.
            if !bulk_sync {
                self.system.commit_to_store();
            }
            let committed_hook_start = transaction_block.then(Instant::now);
            self.system
                .block_committed_with_context(block.as_ref(), persist_context);
            if let Some(start) = committed_hook_start {
                stats.transaction_committed_hook_elapsed += start.elapsed();
            }

            // Cold-start bulk sync imports a trusted local chain.acc package,
            // so it stays on canonical state transitions only. Live import and
            // peer-relay paths still mirror C# MemPool.UpdatePoolForBlockPersisted
            // per block.
            if !bulk_sync {
                self.mempool.block_persisted(block.as_ref());
                self.reverify_mempool_after_persist(
                    index,
                    self.system.settings().max_transactions_per_block as usize,
                );
            }
            if !bulk_sync {
                self.header_cache.remove_up_to(index);
            }

            if !bulk_sync {
                let drained = self.handle_drain_unverified_blocks().await;
                if drained > 0 {
                    debug!(target: "neo", drained, "drained parked unverified blocks after import");
                }
            }
            imported += 1;
            last_imported_height = Some(index);
            position += 1;
        }
        if bulk_sync {
            if imported > 0 {
                let finalization_start = Instant::now();
                let commit_handlers_start = Instant::now();
                if let Err(error) = self.system.flush_bulk_sync_commit_handlers() {
                    warn!(
                        target: "neo",
                        imported,
                        error = %error,
                        "bulk import finalization failed before durable store commit"
                    );
                    stats.finalization_commit_handlers_elapsed += commit_handlers_start.elapsed();
                    stats.finalization_elapsed += finalization_start.elapsed();
                    return ImportBlocksReply::failed_with_stats(imported, stats, error);
                }
                stats.finalization_commit_handlers_elapsed += commit_handlers_start.elapsed();
                let store_commit_start = Instant::now();
                self.system.commit_to_store();
                stats.finalization_store_commit_elapsed += store_commit_start.elapsed();
                stats.finalization_elapsed += finalization_start.elapsed();
            }
            if let Some(height) = last_imported_height {
                self.header_cache.remove_up_to(height);
                let removed = self.remove_parked_blocks_up_to(height);
                if removed > 0 {
                    debug!(
                        target: "neo",
                        removed,
                        height,
                        "removed stale parked blocks after bulk import"
                    );
                }
            }
            let drained = self.handle_drain_unverified_blocks().await;
            if drained > 0 {
                debug!(target: "neo", drained, "drained parked unverified blocks after bulk import");
            }
        }
        ImportBlocksReply::ok_with_stats(imported, stats)
    }

    /// Handle a [`BlockchainCommand::InventoryBlock`] command.
    pub(crate) async fn handle_block_inventory(
        &self,
        block: Arc<Block>,
        relay: bool,
        pre_verified: bool,
    ) -> CoreResult<()> {
        self.handle_block_inventory_without_drain(block, relay, pre_verified, false)
            .await?;
        let drained = self.handle_drain_unverified_blocks().await;
        if drained > 0 {
            debug!(target: "neo", drained, "drained parked unverified blocks");
        }
        Ok(())
    }

    /// Handle a contiguous burst of inventory blocks without requiring one
    /// command-channel round trip per block. Each block still goes through the
    /// normal inventory validation/persist path.
    pub(crate) async fn handle_block_inventory_batch(
        &self,
        blocks: Vec<Arc<Block>>,
        relay: bool,
        pre_verified: bool,
    ) -> usize {
        let mut imported = 0usize;
        let mut direct_imported = 0usize;
        for block in blocks {
            let before_height = self.ledger.current_height();
            match self
                .handle_block_inventory_without_drain(block, relay, pre_verified, true)
                .await
            {
                Ok(()) => {}
                Err(error) => {
                    warn!(target: "neo", %error, "inventory block rejected in batch");
                    continue;
                }
            }
            let current_height = self.ledger.current_height();
            if current_height > before_height {
                imported += 1;
                direct_imported += 1;
            }
        }
        if direct_imported > 0 {
            self.system.commit_to_store();
        }
        let drained = self.handle_drain_unverified_blocks().await;
        if drained > 0 {
            debug!(target: "neo", drained, "drained parked unverified blocks after inventory batch");
            imported += drained;
        }
        imported
    }

    async fn handle_block_inventory_without_drain(
        &self,
        block: Arc<Block>,
        relay: bool,
        pre_verified: bool,
        defer_store_commit: bool,
    ) -> CoreResult<()> {
        let index = block.index();
        let current_height = self.ledger.current_height();

        if index <= current_height {
            debug!(
                target: "neo",
                index,
                current_height,
                "inventory block already persisted"
            );
            return Ok(());
        }

        if index > current_height + 1 {
            let hash = Self::try_block_hash(block.as_ref())?;
            self.ensure_block_matches_cached_header(index, hash)?;
            debug!(
                target: "neo",
                index,
                current_height,
                "inventory block is ahead of the chain tip; parking"
            );
            self.park_unverified_block(block, relay, pre_verified);
            return Ok(());
        }

        self.persist_next_expected_block_with_commit_policy(
            block,
            relay,
            pre_verified,
            defer_store_commit,
        )
        .await
    }

    pub(crate) async fn persist_next_expected_block(
        &self,
        block: Arc<Block>,
        relay: bool,
        pre_verified: bool,
    ) -> CoreResult<()> {
        self.persist_next_expected_block_with_commit_policy(block, relay, pre_verified, false)
            .await
    }

    async fn persist_next_expected_block_with_commit_policy(
        &self,
        block: Arc<Block>,
        relay: bool,
        pre_verified: bool,
        defer_store_commit: bool,
    ) -> CoreResult<()> {
        let wall_start = std::time::Instant::now();
        let index = block.index();
        let hash = Self::try_block_hash(block.as_ref())?;
        let current_height = self.ledger.current_height();

        if let Some(stop_height) = self.stop_at_height {
            if index > stop_height {
                return Err(CoreError::other(format!(
                    "validation stop height {stop_height} reached; refusing block {index}"
                )));
            }
        }

        if index <= current_height {
            return Ok(());
        }

        let after_hash = wall_start.elapsed();
        if index != current_height + 1 {
            return Err(CoreError::other(format!(
                "block {index} is not the next expected height {}",
                current_height + 1
            )));
        }

        // C# Blockchain.OnNewBlock: when the header-first path has already
        // accepted a header for this height, the full block must be byte-for-byte
        // the body for that header (same unsigned-header hash). A competing block
        // with a valid witness but a different hash is invalid, not a fork choice.
        self.ensure_block_matches_cached_header(index, hash)?;

        // Stateless block-integrity pre-checks before persisting a peer-relayed
        // block (the structural half of C# `Block.Verify`): version, transaction
        // merkle root, and duplicate transaction hashes.
        if let Err(error) =
            crate::block_validation::BlockValidator::validate_import_integrity(block.as_ref())
        {
            return Err(CoreError::other(format!(
                "block {index} failed import-integrity validation: {error}"
            )));
        }

        // C# Header.Verify (Blockchain.OnNewBlock runs block.Verify before
        // Persist): a peer-relayed block must pass the structural header checks
        // and carry a consensus witness that satisfies the PREVIOUS block's
        // NextConsensus (the committee/validators multisig address). Locally
        // produced (pre-verified) blocks from the consensus driver skip this.
        //
        // Trusted offline imports can explicitly set `verify = false` through
        // the import path. Peer-relayed blocks do not get that shortcut: live
        // sync must use the same consensus-witness rule at height 1 and at
        // height 10 million.
        if !pre_verified {
            self.verify_consensus_witness_against_store(block.as_ref())?;
        }

        let after_verify = wall_start.elapsed();

        // C# Blockchain.OnNewBlock → Persist(block): the native-contract
        // state transition runs before the block becomes the new tip.
        if !self.persist_block_sequence(Arc::clone(&block)).await {
            return Err(CoreError::other(format!(
                "native persistence pipeline failed for block {index}"
            )));
        }

        let after_persist = wall_start.elapsed();

        if let Err(error) = self.ledger.insert_block_arc(Arc::clone(&block)) {
            return Err(CoreError::other(format!("ledger insert: {error}")));
        }

        if !defer_store_commit {
            // Flush the block's native-persist writes through to the durable store.
            // Per-block commit is memory-safe (no unbounded DataCache growth) and
            // with fast-sync store mode (WAL disabled) the RocksDB write is only
            // ~17µs — negligible compared to the 0.5ms native-contract persist.
            self.system.commit_to_store();
        }
        let after_commit = wall_start.elapsed();
        self.system.block_committed(block.as_ref());

        // Per-block timing breakdown (debug-level). Shows where wall-clock
        // time goes: hash, verify (signature), persist (native contracts),
        // or commit (RocksDB write). Enable with RUST_LOG=neo::sync=debug.
        let total_us = neo_runtime::time::elapsed_us(after_commit);
        let verify_us = neo_runtime::time::elapsed_us(after_verify - after_hash);
        let persist_us = neo_runtime::time::elapsed_us(after_persist - after_verify);
        let commit_us = neo_runtime::time::elapsed_us(after_commit - after_persist);
        debug!(
            target: "neo::sync",
            index,
            hash_us = neo_runtime::time::elapsed_us(after_hash),
            verify_us,
            persist_us,
            commit_us,
            total_us,
            "block persist timing"
        );
        // Feed the sync metrics system for the Prometheus /metrics endpoint
        // and the rolling throughput window.
        neo_runtime::sync_metrics::record_block(
            index as u64,
            verify_us,
            persist_us,
            commit_us,
            total_us,
        );

        // C# Blockchain.Persist → MemPool.UpdatePoolForBlockPersisted: drop the
        // block's transactions from the pool and evict pooled conflicts, so
        // mined txs are no longer served to peers or re-proposed by consensus.
        self.mempool.block_persisted(block.as_ref());
        self.reverify_mempool_after_persist(
            index,
            self.system.settings().max_transactions_per_block as usize,
        );

        self.event_tx
            .send(crate::RuntimeEvent::Imported {
                hash,
                height: index,
                timestamp: block.header.timestamp(),
            })
            .ok();
        self.header_cache.remove_up_to(index);

        let _ = relay; // relay broadcast is handled by the network service
        Ok(())
    }

    /// Handle a [`BlockchainCommand::RelayResult`] notification.
    pub(crate) async fn handle_relay_result(&self, _result: RelayResult) {}

    /// Compute the hash of a block. Returns an error string when the
    /// header cannot be hashed (e.g. because it is missing).
    pub(crate) fn try_block_hash(block: &Block) -> CoreResult<neo_primitives::UInt256> {
        let header = block.header.clone();
        header
            .try_hash()
            .map_err(|err| CoreError::other(format!("hash computation failed: {err}")))
    }
}

// =============================================================================
// Tests
// =============================================================================
#[cfg(test)]
#[path = "../tests/pipeline/handlers.rs"]
mod tests;
