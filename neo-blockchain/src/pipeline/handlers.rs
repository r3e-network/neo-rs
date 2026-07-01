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

use neo_error::{CoreError, CoreResult};
use neo_payloads::{block::Block, extensible_payload::ExtensiblePayload, header::Header};
use neo_primitives::verify_result::VerifyResult;
use tracing::{debug, warn};

use crate::PreverifyCompleted;
use crate::block_processing::BatchPersistResources;
use crate::command::ImportBlocksReply;
use crate::empty_block_fast_forward::{
    MAX_EMPTY_BLOCK_FAST_FORWARD_BLOCKS, stage_empty_block_fast_forward,
};
use crate::import::Import;
use crate::internal::ImportDisposition;
use crate::native_persist::NativePersistOptions;
use crate::persist_completed::PersistCompleted;
use crate::relay_result::RelayResult;
use crate::reverify::Reverify;
use crate::service::{BlockchainService, MempoolLike};
use crate::service_context::BlockPersistContext;

#[path = "../handlers/transactions.rs"]
mod transactions;

impl<S, M> BlockchainService<S, M>
where
    S: crate::service_context::SystemContext,
    M: MempoolLike,
{
    fn verify_header_against_store(&self, block: &Block) -> CoreResult<()> {
        let settings = self.system.settings();
        let snapshot = self.system.store_snapshot().ok_or_else(|| {
            CoreError::other(format!(
                "block {}: store snapshot unavailable",
                block.index()
            ))
        })?;
        self.verify_header_against_snapshot(block, settings.as_ref(), snapshot.as_ref())
    }

    fn verify_header_against_snapshot(
        &self,
        block: &Block,
        settings: &neo_config::ProtocolSettings,
        snapshot: &neo_storage::DataCache,
    ) -> CoreResult<()> {
        let index = block.index();
        if i32::from(block.header.primary_index()) >= settings.validators_count {
            return Err(CoreError::other(format!(
                "block {index}: primary index out of range"
            )));
        }

        let prev = neo_native_contracts::LedgerContract::new()
            .get_trimmed_block(snapshot, block.header.prev_hash())
            .ok()
            .flatten()
            .ok_or_else(|| CoreError::other(format!("block {index}: previous block not found")))?;
        if prev.index() + 1 != index {
            return Err(CoreError::other(format!(
                "block {index}: previous block index mismatch"
            )));
        }
        if prev.hash() != *block.header.prev_hash() {
            return Err(CoreError::other(format!(
                "block {index}: previous block hash mismatch"
            )));
        }
        if block.header.timestamp() <= prev.header.timestamp() {
            return Err(CoreError::other(format!(
                "block {index}: timestamp not after previous block"
            )));
        }

        let next_consensus = *prev.header.next_consensus();
        if neo_execution::Helper::verify_witness(
            &block.header,
            settings,
            snapshot,
            &next_consensus,
            &block.header.witness,
            300_000_000,
        )
        .is_err()
        {
            return Err(CoreError::other(format!(
                "block {index}: consensus witness verification failed"
            )));
        }
        Ok(())
    }

    fn verify_header_with_batch_resources(
        &self,
        block: &Block,
        resources: &BatchPersistResources,
    ) -> CoreResult<()> {
        self.verify_header_against_snapshot(
            block,
            resources.settings.as_ref(),
            resources.snapshot.as_ref(),
        )
    }

    fn collect_empty_fast_forward_run(
        blocks: &[Block],
        start_position: usize,
        current_height: u32,
        settings: &neo_config::ProtocolSettings,
        resources: &crate::native_persist::NativePersistResources,
    ) -> Vec<Arc<Block>> {
        let committee_count = settings.committee_members_count();
        if committee_count == 0 {
            return Vec::new();
        }

        let mut run = Vec::new();
        for block in blocks.iter().skip(start_position) {
            if run.len() >= MAX_EMPTY_BLOCK_FAST_FORWARD_BLOCKS {
                break;
            }
            let expected = current_height.saturating_add(1 + run.len() as u32);
            let height = block.index();
            if height != expected {
                break;
            }
            if !block.transactions.is_empty()
                || block.header.merkle_root() != &neo_primitives::UInt256::zero()
            {
                break;
            }
            if height % (committee_count as u32) == 0 {
                break;
            }
            let native_cut = resources.contracts().iter().any(|contract| {
                let (initialize, _hardforks) = contract.is_initialize_block(settings, height);
                initialize
                    || (contract.is_active(settings, height)
                        && !contract.supports_empty_block_fast_forward())
            });
            if native_cut {
                break;
            }
            run.push(Arc::new(block.clone()));
        }
        run
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

    /// Handle a [`BlockchainCommand::PersistCompleted`]: update hot ledger
    /// caches, evict persisted transactions from the mempool cache, flush the
    /// durable store, and broadcast the persistence event.
    pub(crate) async fn handle_persist_completed(&self, persist: PersistCompleted) {
        let PersistCompleted { block } = persist;
        let index = block.index();
        let hash = match Self::try_block_hash(block.as_ref()) {
            Ok(hash) => hash,
            Err(error) => {
                warn!(
                    target: "neo",
                    error = %error,
                    index,
                    "persist completed block hash computation failed"
                );
                return;
            }
        };
        debug!(
            target: "neo",
            index,
            tx_count = block.transactions.len(),
            "persist completed for block"
        );

        if let Err(error) = self.ledger.insert_block_arc(Arc::clone(&block)) {
            warn!(
                target: "neo",
                %error,
                index,
                "failed to insert persisted block into ledger cache"
            );
        }

        for transaction in &block.transactions {
            if let Ok(hash) = transaction.try_hash() {
                self.ledger.remove_transaction(&hash);
            }
        }

        self.header_cache.remove_up_to(index);
        // Flush the persisted state through to the durable backing store
        // (C# snapshot.Commit() at the end of Blockchain.Persist).
        self.system.commit_to_store();
        self.system
            .block_committed_with_context(block.as_ref(), BlockPersistContext::live());
        self.event_tx
            .send(crate::RuntimeEvent::Imported {
                hash,
                height: index,
                timestamp: block.header.timestamp(),
            })
            .ok();
    }

    /// Handle a [`BlockchainCommand::Headers`] batch.
    ///
    /// C# `Blockchain.OnNewHeaders`: each header must chain onto the previous
    /// one and verify (`Header.Verify(settings, snapshot, headerCache)`) before
    /// it is cached; verification failure stops the batch (the C# `break`),
    /// keeping the valid prefix. The anchor for the first header is the last
    /// cached header, or the ledger tip when the cache is empty.
    pub(crate) fn handle_headers(&self, headers: Vec<Header>) {
        if headers.is_empty() {
            return;
        }

        let snapshot = self.system.store_snapshot();
        let settings = self.system.settings();
        let ledger = neo_native_contracts::LedgerContract::new();

        // C# verification anchor: HeaderCache.Last, else the ledger tip block.
        let mut prev: Option<Header> = self.header_cache.last();
        if prev.is_none() {
            if let Some(snap) = &snapshot {
                if let Ok(tip_hash) = ledger.current_hash(snap) {
                    prev = ledger
                        .get_trimmed_block(snap, &tip_hash)
                        .ok()
                        .flatten()
                        .map(|trimmed| trimmed.header);
                }
            }
        }

        let mut header_height = prev
            .as_ref()
            .map(|h| h.index())
            .unwrap_or_else(|| self.ledger.current_height());

        for header in headers.into_iter() {
            let index = header.index();
            if index <= header_height {
                continue;
            }

            if index != header_height + 1 {
                break;
            }

            // C# Header.Verify(settings, snapshot, headerCache): primary index in
            // range, links onto the anchor, timestamp strictly increases, and the
            // consensus witness satisfies the anchor's NextConsensus (3-GAS cap).
            // Skipped only when no store snapshot is available (no anchor to
            // verify against — e.g. header-only unit fixtures).
            if let (Some(snap), Some(prev_header)) = (&snapshot, &prev) {
                if i32::from(header.primary_index()) >= settings.validators_count {
                    break;
                }
                if header.prev_hash() != &prev_header.hash() {
                    break;
                }
                if header.timestamp() <= prev_header.timestamp() {
                    break;
                }
                let next_consensus = *prev_header.next_consensus();
                if neo_execution::Helper::verify_witness(
                    &header,
                    settings.as_ref(),
                    snap,
                    &next_consensus,
                    &header.witness,
                    300_000_000,
                )
                .is_err()
                {
                    break;
                }
            }

            if !self.header_cache.add(header.clone()) {
                break;
            }

            header_height = index;
            prev = Some(header);
        }
    }

    /// Handle a [`BlockchainCommand::Import`] request.
    pub(crate) async fn handle_import(&self, import: Import) -> ImportBlocksReply {
        let mut imported = 0usize;
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
            let block = blocks[position].clone();
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
                    return ImportBlocksReply::ok(imported);
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
                        return ImportBlocksReply::ok(imported);
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
                            for fast_block in &run {
                                if let Err(error) =
                                    self.ledger.insert_block_arc(Arc::clone(fast_block))
                                {
                                    warn!(
                                        target: "neo",
                                        %error,
                                        height = fast_block.index(),
                                        "failed to import fast-forwarded block into ledger cache"
                                    );
                                    return ImportBlocksReply::ok(imported);
                                }
                                imported += 1;
                                last_imported_height = Some(fast_block.index());
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
                    self.verify_header_with_batch_resources(&block, resources)
                } else {
                    self.verify_header_against_store(&block)
                };
                if let Err(error) = verify_result {
                    warn!(
                        target: "neo",
                        %error,
                        height = index,
                        "import aborted: block verification failed"
                    );
                    return ImportBlocksReply::ok(imported);
                }
            }

            // C# Blockchain.OnImport runs `Persist(block)` — the state
            // transition — before the block becomes the new tip.
            let block = Arc::new(block);
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
                return ImportBlocksReply::ok(imported);
            }

            if let Err(error) = self.ledger.insert_block_arc(Arc::clone(&block)) {
                warn!(
                    target: "neo",
                    %error,
                    height = index,
                    "failed to import block into ledger cache"
                );
                return ImportBlocksReply::ok(imported);
            }

            // Normal live imports flush each block immediately. Trusted
            // bulk-sync imports keep staging into the shared snapshot and flush
            // once after the accepted batch, avoiding one RocksDB commit per
            // block while preserving per-block native/state transitions.
            if !bulk_sync {
                self.system.commit_to_store();
            }
            self.system
                .block_committed_with_context(block.as_ref(), persist_context);

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
                if let Err(error) = self.system.flush_bulk_sync_commit_handlers() {
                    warn!(
                        target: "neo",
                        imported,
                        error = %error,
                        "bulk import finalization failed before durable store commit"
                    );
                    return ImportBlocksReply::failed(imported, error);
                }
                self.system.commit_to_store();
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
        ImportBlocksReply::ok(imported)
    }

    /// Handle a [`BlockchainCommand::Reverify`] request.
    pub(crate) async fn handle_reverify(&self, reverify: Reverify) {
        for item in reverify.inventories {
            match item.payload {
                crate::inventory_payload::InventoryPayload::Block(block) => {
                    let _ = self
                        .handle_block_inventory(Arc::new(*block), false, false)
                        .await;
                }
                crate::inventory_payload::InventoryPayload::Transaction(tx) => {
                    let _ = self.on_new_transaction(&tx);
                }
                crate::inventory_payload::InventoryPayload::Extensible(payload) => {
                    let _ = self.handle_extensible_inventory(*payload, false).await;
                }
                crate::inventory_payload::InventoryPayload::Raw(_, _) => {
                    // Raw payloads are decoded before they reach the service
                    // path, so this compatibility branch is a no-op.
                }
            }
        }
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
        // count, merkle root, and duplicate transactions.
        if let Err(error) =
            crate::block_validation::BlockValidator::validate_block_version(block.version())
        {
            return Err(CoreError::other(format!(
                "block {index} has an invalid version: {error}"
            )));
        }
        // C# Block.Verify delegates to Header.Verify only; MaxTransactionsPerBlock
        // is a dBFT primary-side build limit, not a block-validity rule, so a peer
        // block is NOT rejected on tx count here (matching C# v3.10.0). The P2P
        // message-size limit already bounds how many transactions a block can carry.
        let tx_hashes: Vec<neo_primitives::UInt256> =
            block.transactions.iter().map(|tx| tx.hash()).collect();
        if let Err(error) = crate::block_validation::BlockValidator::validate_merkle_root(
            block.header.merkle_root(),
            &tx_hashes,
        ) {
            return Err(CoreError::other(format!(
                "block {index} failed merkle-root validation: {error}"
            )));
        }
        if let Err(error) =
            crate::block_validation::BlockValidator::validate_no_duplicate_transactions(&tx_hashes)
        {
            return Err(CoreError::other(format!(
                "block {index} has duplicate transactions: {error}"
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
            self.verify_header_against_store(block.as_ref())?;
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
        let total_us = after_commit.as_micros() as u64;
        let verify_us = (after_verify - after_hash).as_micros() as u64;
        let persist_us = (after_persist - after_verify).as_micros() as u64;
        let commit_us = (after_commit - after_persist).as_micros() as u64;
        debug!(
            target: "neo::sync",
            index,
            hash_us = after_hash.as_micros() as u64,
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

    /// Handle a [`BlockchainCommand::InventoryExtensible`] command.
    ///
    /// C# `Blockchain.OnNewExtensiblePayload`: the payload must pass
    /// [`Self::verify_extensible`] (height range, whitelisted sender, witness
    /// execution) before it is cached/relayed.
    pub(crate) async fn handle_extensible_inventory(
        &self,
        mut payload: ExtensiblePayload,
        relay: bool,
    ) -> CoreResult<()> {
        let hash = payload.hash();
        if let Some(snapshot) = self.system.store_snapshot() {
            let settings = self.system.settings();
            Self::verify_extensible(&payload, settings.as_ref(), &snapshot).map_err(|error| {
                CoreError::other(format!("extensible payload rejected: {error}"))
            })?;
        }
        if let Err(error) = self.ledger.insert_extensible(payload) {
            return Err(CoreError::other(format!("ledger insert: {error}")));
        }
        debug!(target: "neo", %hash, relay, "extensible payload accepted");
        Ok(())
    }

    /// C# `ExtensiblePayload.Verify` + `Blockchain.UpdateExtensibleWitnessWhiteList`:
    /// the current height must lie in `[valid_block_start, valid_block_end)`, the
    /// sender must be one of {committee address, next-block-validators BFT address,
    /// each validator's signature hash, state-validators BFT address, each state
    /// validator's signature hash}, and the witness must verify under the 0.06-GAS
    /// cap.
    fn verify_extensible(
        payload: &ExtensiblePayload,
        settings: &neo_config::ProtocolSettings,
        snapshot: &neo_storage::DataCache,
    ) -> CoreResult<()> {
        use neo_payloads::VerifiableExt;

        let ledger = neo_native_contracts::LedgerContract::new();
        let height = ledger
            .current_index(snapshot)
            .map_err(|e| CoreError::other(e.to_string()))?;
        if height < payload.valid_block_start || height >= payload.valid_block_end {
            return Err(CoreError::other(format!(
                "height {height} outside the valid range [{}, {})",
                payload.valid_block_start, payload.valid_block_end
            )));
        }

        let mut whitelist: std::collections::HashSet<neo_primitives::UInt160> =
            std::collections::HashSet::new();
        if let Ok(Some(committee)) = neo_execution::NativeContract::committee_address(
            &neo_native_contracts::NeoToken::new(),
            snapshot,
        ) {
            whitelist.insert(committee);
        }
        let validators = neo_native_contracts::NeoToken::new()
            .next_block_validators(
                snapshot,
                usize::try_from(settings.validators_count).unwrap_or(0),
            )
            .map_err(|e| CoreError::other(e.to_string()))?;
        if !validators.is_empty() {
            whitelist.insert(
                crate::native_persist::bft_address(&validators)
                    .map_err(|e| CoreError::other(e.to_string()))?,
            );
            for validator in &validators {
                whitelist.insert(neo_primitives::UInt160::from_script(
                    &neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(
                        validator.as_bytes(),
                    ),
                ));
            }
        }
        let state_validators = neo_native_contracts::RoleManagement::new()
            .get_designated_by_role_at(snapshot, neo_native_contracts::Role::StateValidator, height)
            .unwrap_or_default();
        if !state_validators.is_empty() {
            whitelist.insert(
                crate::native_persist::bft_address(&state_validators)
                    .map_err(|e| CoreError::other(e.to_string()))?,
            );
            for validator in &state_validators {
                whitelist.insert(neo_primitives::UInt160::from_script(
                    &neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(
                        validator.as_bytes(),
                    ),
                ));
            }
        }
        if !whitelist.contains(&payload.sender) {
            return Err(CoreError::other(
                "sender is not in the extensible witness whitelist",
            ));
        }

        // C# `this.VerifyWitnesses(settings, snapshot, 0_06000000L)`.
        let hashes = payload.script_hashes_for_verifying(snapshot);
        let witnesses = payload.witnesses();
        if hashes.len() != witnesses.len() {
            return Err(CoreError::other("witness count mismatch"));
        }
        let mut remaining_gas = 6_000_000i64;
        for (hash, witness) in hashes.iter().zip(witnesses) {
            match neo_execution::Helper::verify_witness(
                payload,
                settings,
                snapshot,
                hash,
                witness,
                remaining_gas,
            ) {
                Ok(fee) => remaining_gas -= fee,
                Err(error) => {
                    return Err(CoreError::other(format!(
                        "witness verification failed: {error}"
                    )));
                }
            }
        }
        Ok(())
    }

    /// Handle a [`BlockchainCommand::PreverifyCompleted`] command.
    pub(crate) async fn handle_preverify_completed(&self, task: PreverifyCompleted) {
        let hash = match task.transaction.try_hash() {
            Ok(hash) => hash,
            Err(error) => {
                warn!(
                    target: "neo",
                    error = %error,
                    "transaction hash computation failed after preverification"
                );
                return;
            }
        };
        if task.result == VerifyResult::Succeed {
            let result = self.on_new_transaction(&task.transaction);
            debug!(
                target: "neo",
                %hash,
                ?result,
                relay = task.relay,
                "preverified transaction admitted through mempool"
            );
            return;
        }
        debug!(target: "neo", %hash, ?task.result, relay = task.relay, "preverify rejected transaction");
    }

    /// Handle a [`BlockchainCommand::RelayResult`] notification.
    pub(crate) async fn handle_relay_result(&self, _result: RelayResult) {}

    /// Handle a [`BlockchainCommand::Initialize`] command.
    ///
    /// C# `Blockchain.OnInitialize` (Blockchain.cs:197): when the chain
    /// state is uninitialized (`!NativeContract.Ledger.Initialized`),
    /// persist the genesis block — which deploys/initializes the
    /// genesis-active natives (NEO committee cache + total-supply mint,
    /// Oracle price, …) and runs their OnPersist/PostPersist hooks.
    /// Without a store snapshot from the [`SystemContext`] the service cannot
    /// persist genesis and therefore leaves initialization to the caller.
    pub(crate) async fn initialize(&self) {
        if let Some(snapshot) = self.system.store_snapshot() {
            if !crate::native_persist::chain_state_initialized(&snapshot) {
                let settings = self.system.settings();
                match crate::native_persist::genesis_block(settings.as_ref()) {
                    Ok(genesis) => {
                        let genesis = Arc::new(genesis);
                        match crate::native_persist::stage_block_natives(
                            Arc::clone(&snapshot),
                            Arc::clone(&genesis),
                            settings.as_ref(),
                        ) {
                            Ok(staged) => {
                                if !self.system.block_committing(
                                    genesis.as_ref(),
                                    staged.snapshot(),
                                    &staged.outcome.application_executed,
                                ) {
                                    tracing::error!(
                                        target: "neo",
                                        index = genesis.index(),
                                        "genesis committing hook failed"
                                    );
                                    return;
                                }
                                staged.commit();
                                if let Err(error) =
                                    self.ledger.insert_block_arc(Arc::clone(&genesis))
                                {
                                    warn!(
                                        target: "neo",
                                        %error,
                                        "failed to record the genesis block in the ledger cache"
                                    );
                                }
                                // Flush genesis through to the durable store so a
                                // fresh node persists it on disk (not just in-memory).
                                self.system.commit_to_store();
                                self.system.block_committed_with_context(
                                    genesis.as_ref(),
                                    BlockPersistContext::live(),
                                );
                                debug!(
                                    target: "neo",
                                    initialized = ?staged.outcome.initialized,
                                    "genesis block persisted"
                                );
                            }
                            Err(error) => {
                                tracing::error!(
                                    target: "neo",
                                    %error,
                                    "genesis persistence failed"
                                );
                            }
                        }
                    }
                    Err(error) => {
                        tracing::error!(
                            target: "neo",
                            %error,
                            "genesis block construction failed"
                        );
                    }
                }
            }
        }
        debug!(
            target: "neo",
            height = self.ledger.current_height(),
            "blockchain service initialized"
        );
    }

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
