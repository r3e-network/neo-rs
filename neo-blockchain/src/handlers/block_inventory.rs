use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

use neo_error::{CoreError, CoreResult};
use neo_payloads::block::Block;
use neo_runtime::CheckedBlockBatch;
use tracing::{debug, info, warn};

use crate::block_processing::BlockCommitArtifacts;
use crate::internal::BlockIntegrity;
use crate::ledger_provider::BlockProvider;
use crate::pipeline::consensus_witness_stage::ParentHeaderContext;
use crate::pipeline::signature_verification::{
    SignatureVerificationReceipt, SignatureVerificationSubmitError, SignatureVerificationTicket,
};
use crate::service::{BlockchainService, MempoolLike};
use crate::service_context::{BlockPersistContext, SyncBatchCommitPolicy};

impl<S, M> BlockchainService<S, M>
where
    S: crate::service_context::SystemContext,
    M: MempoolLike,
{
    /// Handle a single canonical block with explicit consensus-witness trust.
    pub(crate) async fn handle_block_inventory(
        &self,
        block: Arc<Block>,
        relay: bool,
        consensus_witness_verified: bool,
    ) -> CoreResult<()> {
        self.handle_block_inventory_without_drain(
            block,
            relay,
            consensus_witness_verified,
            BlockIntegrity::Unchecked,
            false,
            BlockPersistContext::live(),
        )
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
    #[cfg(test)]
    pub(crate) async fn handle_block_inventory_batch(
        &self,
        blocks: Vec<Arc<Block>>,
        relay: bool,
        consensus_witness_verified: bool,
    ) -> CoreResult<usize> {
        self.handle_block_inventory_batch_with_integrity(
            blocks,
            relay,
            consensus_witness_verified,
            BlockIntegrity::Unchecked,
        )
        .await
    }

    /// Handle candidates proven to have passed this service's concrete
    /// stateless preflight while retaining normal peer witness verification,
    /// parking, persistence, relay, event, and mempool semantics.
    pub(crate) async fn handle_checked_block_inventory_batch(
        &self,
        checked: CheckedBlockBatch<Arc<Block>, crate::BlockchainHandle>,
        relay: bool,
    ) -> CoreResult<usize> {
        let (blocks, _rejected) = checked.into_parts();
        self.handle_block_inventory_batch_with_integrity(
            blocks,
            relay,
            false,
            BlockIntegrity::Checked,
        )
        .await
    }

    async fn handle_block_inventory_batch_with_integrity(
        &self,
        blocks: Vec<Arc<Block>>,
        relay: bool,
        consensus_witness_verified: bool,
        integrity: BlockIntegrity,
    ) -> CoreResult<usize> {
        let batch_start = Instant::now();
        let durable_height = self.ledger.current_height();
        let range_end = blocks
            .iter()
            .map(|block| block.index())
            .max()
            .unwrap_or(durable_height);
        let commit_policy = if range_end > durable_height {
            self.system
                .sync_batch_commit_policy(durable_height.saturating_add(1), range_end)
        } else {
            SyncBatchCommitPolicy::PerBlock
        };
        let (defer_store_commit, persist_context) = match commit_policy {
            SyncBatchCommitPolicy::PerBlock => (false, BlockPersistContext::live()),
            SyncBatchCommitPolicy::DeferredLive => (true, BlockPersistContext::sync_batch()),
            SyncBatchCommitPolicy::DeferredCatchUp => (true, BlockPersistContext::catch_up()),
        };
        let settings = self.system.settings();
        let mut imported = 0usize;
        let mut direct_imported = 0usize;
        let mut committed_blocks = Vec::new();
        // Keep a bounded ordered window of header tickets.  One ticket leaves
        // all but one worker idle when the configured pool is larger; the
        // queue remains bounded by the pool's own worker + backlog window.
        let mut pending_signatures: VecDeque<(
            Arc<Block>,
            ParentHeaderContext,
            SignatureVerificationTicket,
        )> = VecDeque::new();
        let mut optimistic_pool_enabled = true;
        let mut prefetched_headers = 0usize;
        let mut max_pending_signatures = 0usize;
        for position in 0..blocks.len() {
            let block = Arc::clone(&blocks[position]);
            let current_signature = match pending_signatures.front() {
                Some((candidate, _, _)) if candidate.index() == block.index() => {
                    pending_signatures.pop_front()
                }
                Some(_) => {
                    // The input stopped being the contiguous chain that the
                    // speculative parent contexts were built from.  Those
                    // receipts are no longer useful and must never be used
                    // after the canonical lane takes a different path.
                    pending_signatures.clear();
                    optimistic_pool_enabled = false;
                    None
                }
                None => None,
            };

            // Fill the bounded look-ahead window before executing the current
            // block.  Every ticket is tied to the exact preceding input header;
            // cheap linkage checks prevent speculative work across a gap.
            if optimistic_pool_enabled
                && let Some(pool) = self.optimistic_signature_verification.as_ref()
            {
                while pending_signatures.len() < pool.window() {
                    let Some(next_position) = position
                        .checked_add(1)
                        .and_then(|next| next.checked_add(pending_signatures.len()))
                    else {
                        break;
                    };
                    let Some(next_block) = blocks.get(next_position) else {
                        break;
                    };
                    let Some(parent_block) = blocks.get(next_position.saturating_sub(1)) else {
                        break;
                    };
                    let parent = ParentHeaderContext {
                        hash: parent_block.header.hash(),
                        index: parent_block.index(),
                        timestamp: parent_block.timestamp(),
                        next_consensus: *parent_block.header.next_consensus(),
                    };
                    if next_block.index() != parent.index.saturating_add(1)
                        || next_block.header.prev_hash() != &parent.hash
                        || next_block.timestamp() <= parent.timestamp
                        || i32::from(next_block.primary_index()) >= settings.validators_count
                    {
                        optimistic_pool_enabled = false;
                        break;
                    }
                    let Some(snapshot) = self.system.store_snapshot() else {
                        optimistic_pool_enabled = false;
                        break;
                    };
                    let Some(native_provider) = self.system.native_contract_provider() else {
                        optimistic_pool_enabled = false;
                        break;
                    };
                    match pool.try_submit_header_witness(
                        next_block.header.clone(),
                        parent,
                        Arc::clone(&settings),
                        snapshot,
                        native_provider,
                    ) {
                        Ok(ticket) => {
                            pending_signatures.push_back((Arc::clone(next_block), parent, ticket));
                            prefetched_headers = prefetched_headers.saturating_add(1);
                            max_pending_signatures =
                                max_pending_signatures.max(pending_signatures.len());
                        }
                        Err(SignatureVerificationSubmitError::QueueFull) => break,
                        Err(
                            SignatureVerificationSubmitError::Closed
                            | SignatureVerificationSubmitError::InvalidInput(_),
                        ) => {
                            optimistic_pool_enabled = false;
                            break;
                        }
                    }
                }
            }

            let committed_block = Arc::clone(&block);
            let before_height = self.ledger.current_height();
            let signature_receipt = current_signature.and_then(|(candidate, _parent, ticket)| {
                if candidate.index() != block.index() {
                    return None;
                }
                // The exact header/parent/cache receipt is checked once at the
                // canonical persistence fence below.  Avoid repeating the
                // provider lookup here while still falling back synchronously
                // when the worker did not return a receipt.
                ticket.wait().ok()
            });
            match self
                .handle_block_inventory_without_drain_with_signature(
                    block,
                    relay,
                    consensus_witness_verified,
                    integrity,
                    defer_store_commit,
                    persist_context,
                    signature_receipt,
                )
                .await
            {
                Ok(()) => {}
                Err(error) => {
                    if self.system.should_stop_blockchain_service() {
                        if defer_store_commit {
                            self.system.abort_store_commit();
                            self.ledger.rewind_to(durable_height);
                        }
                        return Err(error);
                    }
                    // A failed canonical step can change the parent frontier
                    // or leave a future block parked. Discard every later
                    // receipt instead of letting it authorize a different
                    // chain shape.
                    pending_signatures.clear();
                    optimistic_pool_enabled = false;
                    warn!(target: "neo", %error, "inventory block rejected in batch");
                    continue;
                }
            }
            let current_height = self.ledger.current_height();
            if current_height > before_height {
                imported += 1;
                direct_imported += 1;
                committed_blocks.push(committed_block);
            }
        }
        if defer_store_commit && direct_imported > 0 {
            if let Err(error) = self.system.commit_to_store() {
                self.ledger.rewind_to(durable_height);
                return Err(CoreError::other(format!(
                    "inventory batch durable store commit failed: {error}"
                )));
            }
            for block in committed_blocks {
                let hash = Self::try_block_hash(block.as_ref())?;
                let artifacts = BlockCommitArtifacts::without_replay_artifacts(None);
                self.publish_persisted_inventory_block(block, hash, artifacts, persist_context)
                    .await?;
            }
        }
        let drained = self.handle_drain_unverified_blocks().await;
        if drained > 0 {
            debug!(target: "neo", drained, "drained parked unverified blocks after inventory batch");
            imported += drained;
        }
        if let Some(pool) = self.optimistic_signature_verification.as_ref()
            && direct_imported > 0
        {
            let elapsed = batch_start.elapsed();
            let metrics = pool.metrics_snapshot();
            info!(
                target: "neo::performance",
                mode = "optimistic_signature_inventory",
                blocks = direct_imported,
                elapsed_ms = elapsed.as_secs_f64() * 1_000.0,
                blocks_per_second = direct_imported as f64 / elapsed.as_secs_f64().max(1e-9),
                signature_submitted = metrics.submitted,
                signature_valid = metrics.valid,
                signature_invalid = metrics.invalid,
                signature_worker_panics = metrics.worker_panics,
                signature_worker_unavailable = metrics.worker_unavailable,
                signature_queue_full = metrics.queue_full,
                signature_queue_closed = metrics.queue_closed,
                signature_prefetched_headers = prefetched_headers,
                signature_max_pending = max_pending_signatures,
                "optimistic inventory batch completed"
            );
        }
        Ok(imported)
    }

    async fn handle_block_inventory_without_drain(
        &self,
        block: Arc<Block>,
        relay: bool,
        consensus_witness_verified: bool,
        integrity: BlockIntegrity,
        defer_store_commit: bool,
        persist_context: BlockPersistContext,
    ) -> CoreResult<()> {
        self.handle_block_inventory_without_drain_with_signature(
            block,
            relay,
            consensus_witness_verified,
            integrity,
            defer_store_commit,
            persist_context,
            None,
        )
        .await
    }

    async fn handle_block_inventory_without_drain_with_signature(
        &self,
        block: Arc<Block>,
        relay: bool,
        consensus_witness_verified: bool,
        integrity: BlockIntegrity,
        defer_store_commit: bool,
        persist_context: BlockPersistContext,
        signature_receipt: Option<SignatureVerificationReceipt>,
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
            self.park_unverified_block(block, relay, consensus_witness_verified, integrity);
            return Ok(());
        }

        self.persist_next_expected_block_with_signature(
            block,
            relay,
            consensus_witness_verified,
            integrity,
            defer_store_commit,
            persist_context,
            signature_receipt,
        )
        .await
    }

    pub(crate) async fn persist_next_expected_block_with_integrity(
        &self,
        block: Arc<Block>,
        relay: bool,
        consensus_witness_verified: bool,
        integrity: BlockIntegrity,
    ) -> CoreResult<()> {
        self.persist_next_expected_block_with_commit_policy(
            block,
            relay,
            consensus_witness_verified,
            integrity,
            false,
            BlockPersistContext::live(),
        )
        .await
    }

    async fn persist_next_expected_block_with_signature(
        &self,
        block: Arc<Block>,
        relay: bool,
        consensus_witness_verified: bool,
        integrity: BlockIntegrity,
        defer_store_commit: bool,
        persist_context: BlockPersistContext,
        signature_receipt: Option<SignatureVerificationReceipt>,
    ) -> CoreResult<()> {
        self.persist_next_expected_block_with_commit_policy_and_signature(
            block,
            relay,
            consensus_witness_verified,
            integrity,
            defer_store_commit,
            persist_context,
            signature_receipt,
        )
        .await
    }

    async fn persist_next_expected_block_with_commit_policy(
        &self,
        block: Arc<Block>,
        relay: bool,
        consensus_witness_verified: bool,
        integrity: BlockIntegrity,
        defer_store_commit: bool,
        persist_context: BlockPersistContext,
    ) -> CoreResult<()> {
        self.persist_next_expected_block_with_commit_policy_and_signature(
            block,
            relay,
            consensus_witness_verified,
            integrity,
            defer_store_commit,
            persist_context,
            None,
        )
        .await
    }

    async fn persist_next_expected_block_with_commit_policy_and_signature(
        &self,
        block: Arc<Block>,
        relay: bool,
        consensus_witness_verified: bool,
        integrity: BlockIntegrity,
        defer_store_commit: bool,
        persist_context: BlockPersistContext,
        signature_receipt: Option<SignatureVerificationReceipt>,
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
        if integrity.requires_check() {
            if let Err(error) =
                crate::block_validation::BlockValidator::validate_import_integrity(block.as_ref())
            {
                return Err(CoreError::other(format!(
                    "block {index} failed import-integrity validation: {error}"
                )));
            }
        }

        // C# Header.Verify (Blockchain.OnNewBlock runs block.Verify before
        // Persist): a peer-relayed block must pass the structural header checks
        // and carry a consensus witness that satisfies the PREVIOUS block's
        // NextConsensus (the committee/validators multisig address). Locally
        // produced blocks submitted through the dedicated consensus capability
        // skip this.
        //
        // Trusted offline imports can explicitly set `verify = false` through
        // the import path. Peer-relayed blocks do not get that shortcut: live
        // sync must use the same consensus-witness rule at height 1 and at
        // height 10 million.
        let receipt_matches = signature_receipt
            .as_ref()
            .is_some_and(|receipt| self.optimistic_signature_receipt_matches(&block, receipt));
        if !consensus_witness_verified && !receipt_matches {
            self.verify_consensus_witness_against_store(block.as_ref())?;
        }

        let after_verify = wall_start.elapsed();

        // C# Blockchain.OnNewBlock → Persist(block): the native-contract
        // state transition runs before the block becomes the new tip.
        let artifacts = self
            .persist_block_sequence_with_context(
                Arc::clone(&block),
                crate::NativePersistOptions {
                    capture_replay_artifacts: !persist_context.skips_live_observers()
                        && self
                            .system
                            .requires_replay_artifacts(block.as_ref(), persist_context),
                },
                persist_context,
            )
            .await
            .map_err(|error| {
                CoreError::other(format!(
                    "native persistence pipeline failed for block {index}: {error}"
                ))
            })?;

        let after_persist = wall_start.elapsed();

        if !defer_store_commit {
            // Flush the block's native-persist writes through to the durable store.
            // Per-block commit bounds DataCache growth; backend-specific live and
            // catch-up durability policy remains inside the configured Store.
            self.system.commit_to_store().map_err(|error| {
                CoreError::other(format!(
                    "block {index} durable store commit failed: {error}"
                ))
            })?;
        }

        self.ledger
            .insert_block_arc_with_hash(Arc::clone(&block), hash);
        let after_commit = wall_start.elapsed();
        if !defer_store_commit {
            self.publish_persisted_inventory_block(
                Arc::clone(&block),
                hash,
                artifacts,
                persist_context,
            )
            .await?;
        }

        // Per-block timing breakdown (debug-level). Shows where wall-clock
        // time goes: hash, verify (signature), persist (native contracts),
        // or commit (durable backend write). Enable with RUST_LOG=neo::sync=debug.
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

        let _ = relay; // relay broadcast is handled by the network service
        Ok(())
    }

    fn optimistic_signature_receipt_matches(
        &self,
        block: &Block,
        receipt: &SignatureVerificationReceipt,
    ) -> bool {
        let Some(snapshot) = self.system.store_snapshot() else {
            return false;
        };
        let parent = self
            .ledger
            .get_block(block.header.prev_hash())
            .map(|parent| parent.header)
            .or_else(|| {
                self.system
                    .ledger_provider(snapshot.as_ref())
                    .header_by_hash(block.header.prev_hash())
                    .ok()
                    .flatten()
            });
        let Some(parent) = parent else {
            return false;
        };
        let parent = ParentHeaderContext {
            hash: parent.hash(),
            index: parent.index(),
            timestamp: parent.timestamp(),
            next_consensus: *parent.next_consensus(),
        };
        let settings = self.system.settings();
        receipt.matches(
            &block.header,
            &parent,
            settings.as_ref(),
            &snapshot.version(),
        )
    }

    async fn publish_persisted_inventory_block(
        &self,
        block: Arc<Block>,
        hash: neo_primitives::UInt256,
        artifacts: BlockCommitArtifacts<S::CacheBacking>,
        persist_context: BlockPersistContext,
    ) -> CoreResult<()> {
        let index = block.index();
        self.system
            .block_finalized(artifacts.into_finalized(Arc::clone(&block), persist_context))
            .await
            .map_err(|error| {
                CoreError::other(format!(
                    "block {index} committed durably but finalized delivery failed: {error}"
                ))
            })?;

        // C# Blockchain.Persist -> MemPool.UpdatePoolForBlockPersisted: drop the
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
        Ok(())
    }
}
