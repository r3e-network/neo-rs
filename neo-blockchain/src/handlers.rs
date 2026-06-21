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
use crate::import::Import;
use crate::internal::ImportDisposition;
use crate::persist_completed::PersistCompleted;
use crate::relay_result::RelayResult;
use crate::reverify::Reverify;
use crate::service::BlockchainService;

mod transactions;

impl BlockchainService {
    fn verify_header_against_store(&self, block: &Block) -> CoreResult<()> {
        let index = block.index();
        let settings = self.system.settings();
        if i32::from(block.header.primary_index()) >= settings.validators_count {
            return Err(CoreError::other(format!(
                "block {index}: primary index out of range"
            )));
        }

        let snapshot = self.system.store_snapshot().ok_or_else(|| {
            CoreError::other(format!("block {index}: store snapshot unavailable"))
        })?;
        let prev = neo_native_contracts::LedgerContract::new()
            .get_trimmed_block(&snapshot, block.header.prev_hash())
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
            settings.as_ref(),
            &snapshot,
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

        if let Err(error) = self.ledger.insert_block((*block).clone()) {
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
        self.system.block_committed(block.as_ref());
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
    pub(crate) async fn handle_import(&self, import: Import) {
        for block in import.blocks {
            let index = block.index();
            let current_height = self.ledger.current_height();
            match ImportDisposition::classify_import_block(current_height, index) {
                ImportDisposition::AlreadySeen => continue,
                ImportDisposition::FutureGap => {
                    warn!(
                        target: "neo",
                        expected = current_height + 1,
                        actual = index,
                        "import block out of sequence"
                    );
                    break;
                }
                ImportDisposition::NextExpected => {}
            }

            if import.verify {
                if let Err(error) = self.verify_header_against_store(&block) {
                    warn!(
                        target: "neo",
                        %error,
                        height = index,
                        "import aborted: block verification failed"
                    );
                    break;
                }
            }

            // C# Blockchain.OnImport runs `Persist(block)` — the state
            // transition — before the block becomes the new tip.
            let block = Arc::new(block);
            if !self.persist_block_sequence(Arc::clone(&block)).await {
                warn!(
                    target: "neo",
                    height = index,
                    "import aborted: native persistence pipeline failed"
                );
                break;
            }

            if let Err(error) = self.ledger.insert_block((*block).clone()) {
                warn!(
                    target: "neo",
                    %error,
                    height = index,
                    "failed to import block into ledger cache"
                );
                break;
            }

            // Flush the block's native-persist writes to the durable store.
            self.system.commit_to_store();
            self.system.block_committed(block.as_ref());

            // Drop the block's transactions from the pool + evict conflicts
            // (C# MemPool.UpdatePoolForBlockPersisted).
            self.mempool.lock().block_persisted(block.as_ref());
            self.reverify_mempool_after_persist(
                index,
                self.system.settings().max_transactions_per_block as usize,
            );
            self.header_cache.remove_up_to(index);

            let drained = self.handle_drain_unverified_blocks().await;
            if drained > 0 {
                debug!(target: "neo", drained, "drained parked unverified blocks after import");
            }
        }
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

        self.persist_next_expected_block(block, relay, pre_verified)
            .await?;
        let drained = self.handle_drain_unverified_blocks().await;
        if drained > 0 {
            debug!(target: "neo", drained, "drained parked unverified blocks");
        }
        Ok(())
    }

    pub(crate) async fn persist_next_expected_block(
        &self,
        block: Arc<Block>,
        relay: bool,
        pre_verified: bool,
    ) -> CoreResult<()> {
        let index = block.index();
        let hash = Self::try_block_hash(block.as_ref())?;
        let current_height = self.ledger.current_height();

        if index <= current_height {
            return Ok(());
        }

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
        if !pre_verified {
            self.verify_header_against_store(block.as_ref())?;
        }

        // C# Blockchain.OnNewBlock → Persist(block): the native-contract
        // state transition runs before the block becomes the new tip.
        if !self.persist_block_sequence(Arc::clone(&block)).await {
            return Err(CoreError::other(format!(
                "native persistence pipeline failed for block {index}"
            )));
        }

        if let Err(error) = self.ledger.insert_block((*block).clone()) {
            return Err(CoreError::other(format!("ledger insert: {error}")));
        }

        // Flush the block's native-persist writes through to the durable store
        // (C# snapshot.Commit() at the end of Blockchain.Persist) so the on-disk
        // tip advances and a restart resumes from here rather than genesis.
        self.system.commit_to_store();
        self.system.block_committed(block.as_ref());

        // C# Blockchain.Persist → MemPool.UpdatePoolForBlockPersisted: drop the
        // block's transactions from the pool and evict pooled conflicts, so
        // mined txs are no longer served to peers or re-proposed by consensus.
        self.mempool.lock().block_persisted(block.as_ref());
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
                        match crate::native_persist::persist_block_natives(
                            Arc::clone(&snapshot),
                            Arc::clone(&genesis),
                            settings.as_ref(),
                        ) {
                            Ok(outcome) => {
                                if !self.system.block_committing(
                                    genesis.as_ref(),
                                    &snapshot,
                                    &outcome.application_executed,
                                ) {
                                    tracing::error!(
                                        target: "neo",
                                        index = genesis.index(),
                                        "genesis committing hook failed"
                                    );
                                    return;
                                }
                                if let Err(error) = self.ledger.insert_block((*genesis).clone()) {
                                    warn!(
                                        target: "neo",
                                        %error,
                                        "failed to record the genesis block in the ledger cache"
                                    );
                                }
                                // Flush genesis through to the durable store so a
                                // fresh node persists it on disk (not just in-memory).
                                self.system.commit_to_store();
                                self.system.block_committed(genesis.as_ref());
                                debug!(
                                    target: "neo",
                                    initialized = ?outcome.initialized,
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
mod tests;
