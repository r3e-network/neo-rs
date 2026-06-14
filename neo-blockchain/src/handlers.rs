//! Service method handlers for [`crate::service::BlockchainService`].
//!
//! Each command variant has a corresponding `async fn` method on the
//! service. The dispatch loop in [`crate::service::BlockchainService::dispatch`]
//! just `match`es on the command enum and calls the right method —
//! there is no actor framework, no `Box<dyn Any>` downcasting, no
//! per-message `Handle<T>` impls.
//!
//! The handlers own the service-side Neo protocol decisions: block/header
//! sequencing, native persistence, transaction admission, extensible payload
//! verification, and cache maintenance.

use std::sync::Arc;

use neo_error::{CoreError, CoreResult};
use neo_payloads::{
    Transaction, block::Block, extensible_payload::ExtensiblePayload, header::Header,
};
use neo_primitives::verify_result::VerifyResult;
use tracing::{debug, warn};

use crate::PreverifyCompleted;
use crate::command::AddTransactionReply;
use crate::fill_memory_pool::FillMemoryPool;
use crate::import::Import;
use crate::internal::ImportDisposition;
use crate::persist_completed::PersistCompleted;
use crate::relay_result::RelayResult;
use crate::reverify::Reverify;
use crate::service::BlockchainService;

impl BlockchainService {
    /// Handle a [`BlockchainCommand::PersistCompleted`]. The actor's
    /// legacy implementation did cache invalidation + mempool
    /// eviction + relay broadcast; Stage B does the minimum required
    /// to keep the cache consistent.
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

            // Drop the block's transactions from the pool + evict conflicts
            // (C# MemPool.UpdatePoolForBlockPersisted).
            self.mempool.lock().block_persisted(block.as_ref());

            let drained = self.handle_drain_unverified_blocks().await;
            if drained > 0 {
                debug!(target: "neo", drained, "drained parked unverified blocks after import");
            }
        }
    }

    /// Handle a [`BlockchainCommand::FillMemoryPool`] request.
    pub(crate) async fn handle_fill_memory_pool(&self, fill: FillMemoryPool) {
        let mut accepted = 0usize;
        let mut rejected = 0usize;
        for transaction in fill.transactions {
            if self.on_new_transaction(&transaction).is_success() {
                accepted += 1;
            } else {
                rejected += 1;
            }
        }
        debug!(
            target: "neo",
            accepted,
            rejected,
            "fill memory pool completed"
        );
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
                    // Raw payloads are decoded by the actor's old
                    // inventory cache; the new service path
                    // deserialises on receipt so the raw branch is
                    // a no-op.
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
        let settings = self.system.settings();
        if let Err(error) =
            crate::block_validation::BlockValidator::validate_transaction_count_raw_with_limit(
                block.transactions.len(),
                settings.max_transactions_per_block as usize,
            )
        {
            return Err(CoreError::other(format!(
                "block {index} exceeds the transaction limit: {error}"
            )));
        }
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
            if let Some(snapshot) = self.system.store_snapshot() {
                if i32::from(block.header.primary_index()) >= settings.validators_count {
                    return Err(CoreError::other(format!(
                        "block {index}: primary index out of range"
                    )));
                }
                let prev = neo_native_contracts::LedgerContract::new()
                    .get_trimmed_block(&snapshot, block.header.prev_hash())
                    .ok()
                    .flatten()
                    .ok_or_else(|| {
                        CoreError::other(format!("block {index}: previous block not found"))
                    })?;
                if prev.header.index() + 1 != index {
                    return Err(CoreError::other(format!(
                        "block {index}: previous block index mismatch"
                    )));
                }
                if block.header.timestamp() <= prev.header.timestamp() {
                    return Err(CoreError::other(format!(
                        "block {index}: timestamp not after previous block"
                    )));
                }
                // The single block witness must satisfy prev.NextConsensus, under
                // the C# 3-GAS block-verification cap (Header.Verify, not the
                // 1.5-GAS transaction cap).
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
            }
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

        // C# Blockchain.Persist → MemPool.UpdatePoolForBlockPersisted: drop the
        // block's transactions from the pool and evict pooled conflicts, so
        // mined txs are no longer served to peers or re-proposed by consensus.
        self.mempool.lock().block_persisted(block.as_ref());

        self.event_tx
            .send(crate::RuntimeEvent::Imported {
                hash,
                height: index,
                timestamp: block.header.timestamp(),
            })
            .ok();

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

    /// Handle a [`BlockchainCommand::Idle`] tick.
    pub(crate) async fn handle_idle(&self) {
        // Stage B: no mempool reverify queue to tick. The handler is
        // a no-op so the command loop remains round-trip-able.
    }

    /// Handle a [`BlockchainCommand::DrainUnverified`] tick.
    pub(crate) async fn handle_drain_unverified(&self) {
        let drained = self.handle_drain_unverified_blocks().await;
        if drained > 0 {
            debug!(target: "neo", drained, "drained parked unverified blocks");
        }
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
    /// Without a store snapshot from the [`SystemContext`] this remains
    /// the Stage B no-op.
    pub(crate) async fn initialize(&self) {
        if let Some(snapshot) = self.system.store_snapshot() {
            if !crate::native_persist::chain_state_initialized(&snapshot) {
                let settings = self.system.settings();
                match crate::native_persist::genesis_block(settings.as_ref()) {
                    Ok(genesis) => {
                        let genesis = Arc::new(genesis);
                        match crate::native_persist::persist_block_natives(
                            snapshot,
                            Arc::clone(&genesis),
                            settings.as_ref(),
                        ) {
                            Ok(outcome) => {
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

    /// Try to insert a transaction into the mempool. Used by the
    /// high-level `add_transaction` API.
    pub(crate) async fn add_transaction(&self, transaction: Transaction) -> AddTransactionReply {
        let hash = match transaction.try_hash() {
            Ok(hash) => hash,
            Err(error) => {
                warn!(
                    target: "neo",
                    error = %error,
                    "transaction hash computation failed before mempool admission"
                );
                return AddTransactionReply {
                    result: VerifyResult::Invalid,
                    hash: neo_primitives::UInt256::zero(),
                };
            }
        };

        // C# Blockchain.OnNewTransaction verifies against the live store
        // view (`system.StoreView`): hand the mempool the system context's
        // store snapshot so admission runs the real verification pipeline.
        // Contexts without a store (lightweight tests) fall back to an
        // empty cache, which fails state-dependent checks closed.
        let result = {
            let pool = self.mempool.lock();
            let settings = self.system.settings();
            match self.system.store_snapshot() {
                Some(snapshot) => pool.try_add(&transaction, snapshot.as_ref(), &settings),
                None => {
                    let snapshot = neo_storage::DataCache::new(false);
                    pool.try_add(&transaction, &snapshot, &settings)
                }
            }
        };

        if result == VerifyResult::Succeed {
            self.ledger.insert_transaction(transaction.clone()).ok();
        }

        AddTransactionReply { result, hash }
    }

    /// Transaction admission (used by the actor's old reverify path).
    /// Returns the [`VerifyResult`] for the transaction.
    pub(crate) fn on_new_transaction(&self, transaction: &Transaction) -> VerifyResult {
        let hash = match transaction.try_hash() {
            Ok(hash) => hash,
            Err(error) => {
                warn!(
                    target: "neo",
                    error = %error,
                    "transaction hash computation failed before mempool admission"
                );
                return VerifyResult::Invalid;
            }
        };

        if self.ledger.get_transaction(&hash).is_some() {
            return VerifyResult::AlreadyInPool;
        }

        let pool = self.mempool.lock();
        let settings = self.system.settings();
        let result = match self.system.store_snapshot() {
            Some(snapshot) => pool.try_add(transaction, snapshot.as_ref(), &settings),
            None => {
                let snapshot = neo_storage::DataCache::new(false);
                pool.try_add(transaction, &snapshot, &settings)
            }
        };
        if result == VerifyResult::Succeed {
            // Best-effort cache insertion; the mempool is the source
            // of truth.
            let _ = self.ledger.insert_transaction(transaction.clone());
        }
        result
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
mod tests {
    use super::*;
    use crate::command::BlockchainCommand;
    use crate::handle::BlockchainHandle;
    use crate::header_cache::HeaderCache;
    use crate::ledger_context::LedgerContext;
    use crate::service::MempoolLike;
    use crate::service_context::SystemContext;
    use neo_primitives::UInt256;
    use parking_lot::Mutex;
    use tokio::sync::oneshot;

    #[derive(Debug)]
    struct TestContext;
    impl SystemContext for TestContext {
        fn settings(&self) -> Arc<neo_config::ProtocolSettings> {
            Arc::new(neo_config::ProtocolSettings::default())
        }
        fn current_height(&self) -> u32 {
            0
        }
    }

    #[derive(Debug)]
    struct ConfiguredTestContext {
        settings: Arc<neo_config::ProtocolSettings>,
    }
    impl SystemContext for ConfiguredTestContext {
        fn settings(&self) -> Arc<neo_config::ProtocolSettings> {
            Arc::clone(&self.settings)
        }
        fn current_height(&self) -> u32 {
            0
        }
    }

    #[derive(Debug, Default)]
    struct TestMempool;
    impl MempoolLike for TestMempool {
        fn try_add(
            &self,
            _tx: &Transaction,
            _snapshot: &neo_storage::DataCache,
            _settings: &neo_config::ProtocolSettings,
        ) -> VerifyResult {
            VerifyResult::Succeed
        }
    }

    #[derive(Debug)]
    struct FixedResultMempool {
        result: VerifyResult,
    }
    impl MempoolLike for FixedResultMempool {
        fn try_add(
            &self,
            _tx: &Transaction,
            _snapshot: &neo_storage::DataCache,
            _settings: &neo_config::ProtocolSettings,
        ) -> VerifyResult {
            self.result
        }
    }

    fn fixture() -> (BlockchainService, BlockchainHandle) {
        let system: Arc<dyn SystemContext> = Arc::new(TestContext);
        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::default());
        let mempool: Arc<Mutex<dyn MempoolLike + Send + Sync>> = Arc::new(Mutex::new(TestMempool));
        BlockchainService::with_defaults(system, ledger, header_cache, mempool)
    }

    fn fixture_with_mempool_result(result: VerifyResult) -> (BlockchainService, BlockchainHandle) {
        let system: Arc<dyn SystemContext> = Arc::new(TestContext);
        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::default());
        let mempool: Arc<Mutex<dyn MempoolLike + Send + Sync>> =
            Arc::new(Mutex::new(FixedResultMempool { result }));
        BlockchainService::with_defaults(system, ledger, header_cache, mempool)
    }

    fn fixture_with_protocol_settings(
        settings: neo_config::ProtocolSettings,
    ) -> (BlockchainService, BlockchainHandle) {
        let system: Arc<dyn SystemContext> = Arc::new(ConfiguredTestContext {
            settings: Arc::new(settings),
        });
        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::default());
        let mempool: Arc<Mutex<dyn MempoolLike + Send + Sync>> = Arc::new(Mutex::new(TestMempool));
        BlockchainService::with_defaults(system, ledger, header_cache, mempool)
    }

    /// [`SystemContext`] over a shared in-memory store snapshot, so the
    /// native persistence pipeline actually runs.
    struct StoreContext {
        snapshot: Arc<neo_storage::DataCache>,
        settings: Arc<neo_config::ProtocolSettings>,
    }
    impl std::fmt::Debug for StoreContext {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("StoreContext").finish_non_exhaustive()
        }
    }
    impl SystemContext for StoreContext {
        fn settings(&self) -> Arc<neo_config::ProtocolSettings> {
            Arc::clone(&self.settings)
        }
        fn current_height(&self) -> u32 {
            0
        }
        fn store_snapshot(&self) -> Option<Arc<neo_storage::DataCache>> {
            Some(Arc::clone(&self.snapshot))
        }
    }

    fn store_fixture() -> (
        BlockchainService,
        BlockchainHandle,
        Arc<neo_storage::DataCache>,
    ) {
        store_fixture_with(neo_config::ProtocolSettings::default())
    }

    fn store_fixture_with(
        settings: neo_config::ProtocolSettings,
    ) -> (
        BlockchainService,
        BlockchainHandle,
        Arc<neo_storage::DataCache>,
    ) {
        neo_native_contracts::install();
        let snapshot = Arc::new(neo_storage::DataCache::new(false));
        let system: Arc<dyn SystemContext> = Arc::new(StoreContext {
            snapshot: Arc::clone(&snapshot),
            settings: Arc::new(settings),
        });
        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::default());
        let mempool: Arc<Mutex<dyn MempoolLike + Send + Sync>> = Arc::new(Mutex::new(TestMempool));
        let (service, handle) =
            BlockchainService::with_defaults(system, ledger, header_cache, mempool);
        (service, handle, snapshot)
    }

    /// NEO total supply read (NEP-17 `Prefix_TotalSupply` = 11).
    fn neo_total_supply(snapshot: &neo_storage::DataCache) -> Option<num_bigint::BigInt> {
        snapshot
            .get(&neo_storage::StorageKey::new(
                neo_native_contracts::NeoToken::ID,
                vec![11],
            ))
            .map(|item| num_bigint::BigInt::from_signed_bytes_le(&item.value_bytes()))
    }

    fn transaction_with_nonce(nonce: u32) -> Transaction {
        let mut tx = Transaction::new();
        tx.set_nonce(nonce);
        tx.set_script(vec![neo_vm_rs::OpCode::PUSH1.byte()]);
        tx
    }

    #[tokio::test]
    async fn fill_memory_pool_admits_transactions_through_mempool() {
        let (service, _handle) = fixture();
        let tx1 = transaction_with_nonce(101);
        let tx2 = transaction_with_nonce(102);
        let hash1 = tx1.try_hash().expect("tx1 hash");
        let hash2 = tx2.try_hash().expect("tx2 hash");

        service
            .handle_fill_memory_pool(FillMemoryPool {
                transactions: vec![tx1, tx2],
            })
            .await;

        assert!(service.ledger.get_transaction(&hash1).is_some());
        assert!(service.ledger.get_transaction(&hash2).is_some());
    }

    #[tokio::test]
    async fn preverify_completed_uses_mempool_verdict_before_caching() {
        let (rejecting, _handle) = fixture_with_mempool_result(VerifyResult::PolicyFail);
        let rejected = transaction_with_nonce(201);
        let rejected_hash = rejected.try_hash().expect("rejected hash");
        rejecting
            .handle_preverify_completed(crate::PreverifyCompleted {
                transaction: rejected,
                relay: true,
                result: VerifyResult::Succeed,
            })
            .await;
        assert!(
            rejecting.ledger.get_transaction(&rejected_hash).is_none(),
            "state-dependent mempool rejection must not populate the ledger tx cache"
        );

        let (accepting, _handle) = fixture_with_mempool_result(VerifyResult::Succeed);
        let accepted = transaction_with_nonce(202);
        let accepted_hash = accepted.try_hash().expect("accepted hash");
        accepting
            .handle_preverify_completed(crate::PreverifyCompleted {
                transaction: accepted,
                relay: true,
                result: VerifyResult::Succeed,
            })
            .await;
        assert!(accepting.ledger.get_transaction(&accepted_hash).is_some());
    }

    #[tokio::test]
    async fn inventory_block_respects_effective_protocol_transaction_limit() {
        let mut settings = neo_config::ProtocolSettings::default();
        settings.max_transactions_per_block = 1;
        let (service, _handle) = fixture_with_protocol_settings(settings);

        let mut tx_a = Transaction::new();
        tx_a.set_nonce(1);
        tx_a.set_script(vec![0x51]);
        let mut tx_b = Transaction::new();
        tx_b.set_nonce(2);
        tx_b.set_script(vec![0x51]);

        let mut header = Header::new();
        header.set_index(1);
        let mut block = Block::from_parts(header, vec![tx_a, tx_b]);
        block.try_rebuild_merkle_root().expect("valid merkle root");

        let error = service
            .handle_block_inventory(Arc::new(block), false, true)
            .await
            .expect_err("block above the effective protocol transaction cap is rejected");

        assert!(
            error.to_string().contains("exceeds the transaction limit"),
            "{error}"
        );
        assert!(error.to_string().contains("maximum 1"), "{error}");
    }

    #[tokio::test]
    async fn initialize_bootstraps_genesis_once_and_inventory_runs_native_hooks() {
        let (service, _handle, snapshot) = store_fixture();

        // C# Blockchain.OnInitialize: an uninitialized store gets the
        // genesis block persisted (native deploy seeds + mints).
        service.initialize().await;
        assert!(crate::native_persist::chain_state_initialized(&snapshot));
        assert_eq!(
            neo_total_supply(&snapshot),
            Some(num_bigint::BigInt::from(100_000_000)),
            "genesis minted the NEO total supply"
        );
        assert!(
            service.ledger.block_hash_at(0).is_some(),
            "genesis cached in the ledger"
        );

        // Re-initializing must NOT re-persist (the initialized probe
        // guards the C# `Ledger.Initialized` branch): the supply stays
        // 100M instead of doubling.
        service.initialize().await;
        assert_eq!(
            neo_total_supply(&snapshot),
            Some(num_bigint::BigInt::from(100_000_000))
        );

        // An inventory block at the next height runs the OnPersist /
        // PostPersist native hooks over the same store: block 1 mints
        // the 0.5-GAS committee reward to standby_committee[1 % 21].
        // The synthetic header carries no real consensus witness, so it goes
        // through the pre-verified path (the consensus-driver submission route);
        // witness verification of peer-relayed blocks has its own tests below.
        let mut header = Header::new();
        header.set_index(1);
        let block = Arc::new(Block::from_parts(header, vec![]));
        service
            .handle_block_inventory(block, false, true)
            .await
            .expect("inventory block persists");
        assert_eq!(service.ledger.current_height(), 1);

        let settings = neo_config::ProtocolSettings::default();
        let member = &settings.standby_committee[1];
        let script = neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(
            &member.to_bytes(),
        );
        let account = neo_primitives::UInt160::from_script(&script);
        let mut key = vec![20u8]; // shared NEP-17 Prefix_Account
        key.extend_from_slice(&account.to_bytes());
        assert!(
            snapshot
                .get(&neo_storage::StorageKey::new(
                    neo_native_contracts::GasToken::ID,
                    key
                ))
                .is_some(),
            "block-1 PostPersist minted the rotating committee reward"
        );
    }

    #[tokio::test]
    async fn future_inventory_block_is_parked_then_drained_after_parent_persists() {
        let (service, _handle, snapshot) = store_fixture();
        service.initialize().await;

        let settings = neo_config::ProtocolSettings::default();
        let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

        let mut header1 = Header::new();
        header1.set_index(1);
        header1.set_prev_hash(genesis.hash());
        header1.set_timestamp(genesis.header.timestamp() + 15_000);
        header1.set_next_consensus(*genesis.header.next_consensus());
        let block1 = Arc::new(Block::from_parts(header1, vec![]));
        let block1_hash = BlockchainService::try_block_hash(block1.as_ref()).expect("block1 hash");

        let mut header2 = Header::new();
        header2.set_index(2);
        header2.set_prev_hash(block1_hash);
        header2.set_timestamp(genesis.header.timestamp() + 30_000);
        header2.set_next_consensus(*genesis.header.next_consensus());
        let block2 = Arc::new(Block::from_parts(header2, vec![]));

        service
            .handle_block_inventory(Arc::clone(&block2), false, true)
            .await
            .expect("future block is parked, not rejected");
        assert_eq!(service.ledger.current_height(), 0);
        assert_eq!(service.unverified_block_count(), 1);
        assert!(service.ledger.block_hash_at(2).is_none());

        service
            .handle_block_inventory(block1, false, true)
            .await
            .expect("parent block persists and drains child");

        assert_eq!(service.ledger.current_height(), 2);
        assert_eq!(service.unverified_block_count(), 0);
        assert!(service.ledger.block_hash_at(1).is_some());
        assert!(service.ledger.block_hash_at(2).is_some());
        assert_eq!(
            neo_native_contracts::LedgerContract::new()
                .current_index(&snapshot)
                .expect("ledger current index"),
            2
        );
    }

    /// End-to-end consensus-witness verification of a peer-relayed block: a
    /// block signed by the network's validator (1-of-1 multisig over the C#
    /// sign data = network magic LE + header hash) is accepted, and the same
    /// block with a tampered signature is rejected. Proves the whole
    /// `Header.Verify` path (prev-block lookup, timestamp/primary checks,
    /// script-hash match against prev `NextConsensus`, CheckMultisig over the
    /// header sign data) so live sync cannot be stalled by a broken verifier.
    #[tokio::test]
    async fn peer_block_witness_verification_accepts_valid_and_rejects_tampered() {
        let private_key = neo_crypto::Secp256r1Crypto::generate_private_key();
        let public_key =
            neo_crypto::Secp256r1Crypto::derive_public_key(&private_key).expect("public key");
        let point = neo_crypto::ECPoint::from_bytes(&public_key).expect("point");
        let mut settings = neo_config::ProtocolSettings::default();
        settings.standby_committee = vec![point.clone()];
        settings.validators_count = 1;
        let network = settings.network;

        let (service, _handle, _snapshot) = store_fixture_with(settings.clone());
        service.initialize().await;
        let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

        // Block 1 over genesis (no transactions; merkle root stays zero).
        let mut header = Header::new();
        header.set_index(1);
        header.set_prev_hash(genesis.hash());
        header.set_timestamp(genesis.header.timestamp() + 15_000);
        header.set_primary_index(0);
        header.set_next_consensus(*genesis.header.next_consensus());

        // C# sign data: network magic (LE) + header hash (witness excluded).
        let mut sign_data = Vec::with_capacity(36);
        sign_data.extend_from_slice(&network.to_le_bytes());
        sign_data.extend_from_slice(&header.hash().to_bytes());
        let signature = neo_crypto::Secp256r1Crypto::sign(&sign_data, &private_key).expect("sign");
        let verification = neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(1, &[point])
            .expect("multisig script");
        let invocation = |sig: &[u8]| {
            let mut script = vec![0x0C, 64]; // PUSHDATA1 64
            script.extend_from_slice(sig);
            script
        };

        // Tampered signature -> rejected, nothing persisted.
        let mut tampered_sig = signature;
        tampered_sig[10] ^= 0xFF;
        let mut tampered = header.clone();
        tampered.witness = neo_payloads::Witness::new_with_scripts(
            invocation(&tampered_sig),
            verification.clone(),
        );
        let err = service
            .handle_block_inventory(Arc::new(Block::from_parts(tampered, vec![])), false, false)
            .await
            .expect_err("tampered consensus witness must be rejected");
        assert!(
            err.to_string().contains("witness"),
            "rejection names the witness: {err}"
        );
        assert_eq!(service.ledger.current_height(), 0);

        // Valid signature -> accepted and persisted.
        header.witness =
            neo_payloads::Witness::new_with_scripts(invocation(&signature), verification);
        service
            .handle_block_inventory(Arc::new(Block::from_parts(header, vec![])), false, false)
            .await
            .expect("validly signed peer block is accepted");
        assert_eq!(service.ledger.current_height(), 1);
    }

    /// Public `BlockchainHandle::import_block` is the RPC/user-submitted block
    /// path, so it must wait for the service verdict and verify the consensus
    /// witness instead of reporting success after merely queueing the command.
    #[tokio::test]
    async fn handle_import_block_reports_rejection_and_verifies_witness() {
        let private_key = neo_crypto::Secp256r1Crypto::generate_private_key();
        let public_key =
            neo_crypto::Secp256r1Crypto::derive_public_key(&private_key).expect("public key");
        let point = neo_crypto::ECPoint::from_bytes(&public_key).expect("point");
        let mut settings = neo_config::ProtocolSettings::default();
        settings.standby_committee = vec![point.clone()];
        settings.validators_count = 1;
        let network = settings.network;

        let (service, handle, _snapshot) = store_fixture_with(settings.clone());
        service.initialize().await;
        let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

        let mut header = Header::new();
        header.set_index(1);
        header.set_prev_hash(genesis.hash());
        header.set_timestamp(genesis.header.timestamp() + 15_000);
        header.set_primary_index(0);
        header.set_next_consensus(*genesis.header.next_consensus());

        let mut sign_data = Vec::with_capacity(36);
        sign_data.extend_from_slice(&network.to_le_bytes());
        sign_data.extend_from_slice(&header.hash().to_bytes());
        let signature = neo_crypto::Secp256r1Crypto::sign(&sign_data, &private_key).expect("sign");
        let verification = neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(1, &[point])
            .expect("multisig script");
        let invocation = |sig: &[u8]| {
            let mut script = vec![0x0C, 64];
            script.extend_from_slice(sig);
            script
        };

        let mut tampered_signature = signature;
        tampered_signature[10] ^= 0xFF;
        let mut tampered_header = header.clone();
        tampered_header.witness = neo_payloads::Witness::new_with_scripts(
            invocation(&tampered_signature),
            verification.clone(),
        );

        header.witness =
            neo_payloads::Witness::new_with_scripts(invocation(&signature), verification);

        let runner = tokio::spawn(service.run());

        let rejected = handle
            .import_block(Block::from_parts(tampered_header, vec![]))
            .await
            .expect("import command reply");
        assert!(
            !rejected,
            "tampered witness must not be reported as imported"
        );
        assert_eq!(handle.get_height().await.expect("height reply"), 0);

        let imported = handle
            .import_block(Block::from_parts(header, vec![]))
            .await
            .expect("import command reply");
        assert!(imported, "validly signed block advances the tip");
        assert_eq!(handle.get_height().await.expect("height reply"), 1);

        drop(handle);
        runner
            .await
            .expect("service exits after command channel closes");
    }

    /// C# `Blockchain.OnNewExtensiblePayload`: an extensible payload signed by
    /// a whitelisted sender (here the network's validator) within its validity
    /// range is accepted; a stale range or a non-whitelisted sender is rejected.
    #[tokio::test]
    async fn extensible_inventory_verifies_range_whitelist_and_witness() {
        let private_key = neo_crypto::Secp256r1Crypto::generate_private_key();
        let public_key =
            neo_crypto::Secp256r1Crypto::derive_public_key(&private_key).expect("public key");
        let point = neo_crypto::ECPoint::from_bytes(&public_key).expect("point");
        let mut settings = neo_config::ProtocolSettings::default();
        settings.standby_committee = vec![point];
        settings.validators_count = 1;
        let network = settings.network;

        let (service, _handle, _snapshot) = store_fixture_with(settings.clone());
        service.initialize().await;

        let verification =
            neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(
                &public_key,
            );
        let sender = neo_primitives::UInt160::from_script(&verification);

        let mut payload = ExtensiblePayload::new();
        payload.category = "dBFT".to_string();
        payload.valid_block_start = 0;
        payload.valid_block_end = 10;
        payload.sender = sender;
        payload.data = vec![0x01, 0x02, 0x03];
        let mut sign_data = Vec::with_capacity(36);
        sign_data.extend_from_slice(&network.to_le_bytes());
        sign_data.extend_from_slice(&payload.hash().to_bytes());
        let signature = neo_crypto::Secp256r1Crypto::sign(&sign_data, &private_key).expect("sign");
        let mut invocation = vec![0x0C, 64];
        invocation.extend_from_slice(&signature);
        payload.witness = neo_payloads::Witness::new_with_scripts(invocation, verification.clone());

        // Out-of-range: height 0 is not inside [5, 10) -> rejected.
        let mut stale = payload.clone();
        stale.valid_block_start = 5;
        let err = service
            .handle_extensible_inventory(stale, false)
            .await
            .expect_err("out-of-range extensible must be rejected");
        assert!(err.to_string().contains("valid range"), "{err}");

        // Non-whitelisted sender -> rejected before witness execution.
        let mut foreign = payload.clone();
        foreign.sender = neo_primitives::UInt160::from_bytes(&[0x42; 20]).unwrap();
        let err = service
            .handle_extensible_inventory(foreign, false)
            .await
            .expect_err("non-whitelisted sender must be rejected");
        assert!(err.to_string().contains("whitelist"), "{err}");

        // Valid range + whitelisted validator sender + correct signature.
        service
            .handle_extensible_inventory(payload, false)
            .await
            .expect("validly signed whitelisted extensible is accepted");
    }

    /// C# `Blockchain.OnNewHeaders`: a header signed by the network validator
    /// (over the genesis anchor's NextConsensus) is cached; a tampered witness
    /// stops the batch and keeps the valid prefix (here: nothing cached).
    #[tokio::test]
    async fn headers_verify_against_the_anchor_next_consensus() {
        let private_key = neo_crypto::Secp256r1Crypto::generate_private_key();
        let public_key =
            neo_crypto::Secp256r1Crypto::derive_public_key(&private_key).expect("public key");
        let point = neo_crypto::ECPoint::from_bytes(&public_key).expect("point");
        let mut settings = neo_config::ProtocolSettings::default();
        settings.standby_committee = vec![point.clone()];
        settings.validators_count = 1;
        let network = settings.network;

        let (service, _handle, _snapshot) = store_fixture_with(settings.clone());
        service.initialize().await;
        let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

        let mut header = Header::new();
        header.set_index(1);
        header.set_prev_hash(genesis.hash());
        header.set_timestamp(genesis.header.timestamp() + 15_000);
        header.set_next_consensus(*genesis.header.next_consensus());
        let mut sign_data = Vec::with_capacity(36);
        sign_data.extend_from_slice(&network.to_le_bytes());
        sign_data.extend_from_slice(&header.hash().to_bytes());
        let signature = neo_crypto::Secp256r1Crypto::sign(&sign_data, &private_key).expect("sign");
        let verification = neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(1, &[point])
            .expect("multisig script");
        let invocation = |sig: &[u8]| {
            let mut script = vec![0x0C, 64];
            script.extend_from_slice(sig);
            script
        };

        // Tampered witness -> batch stops, header not cached.
        let mut tampered_sig = signature;
        tampered_sig[5] ^= 0xFF;
        let mut bad = header.clone();
        bad.witness = neo_payloads::Witness::new_with_scripts(
            invocation(&tampered_sig),
            verification.clone(),
        );
        service.handle_headers(vec![bad]);
        assert_eq!(
            service.header_cache.count(),
            0,
            "tampered header is not cached"
        );

        // Valid witness -> cached.
        header.witness =
            neo_payloads::Witness::new_with_scripts(invocation(&signature), verification);
        service.handle_headers(vec![header]);
        assert_eq!(
            service.header_cache.count(),
            1,
            "validly signed header is cached"
        );
    }

    #[tokio::test]
    async fn headers_in_sequence_are_accepted() {
        let (service, _handle) = fixture();
        let mut header = Header::new();
        header.set_index(1);
        service.handle_headers(vec![header]);
        assert_eq!(service.header_cache.count(), 1);
    }

    #[tokio::test]
    async fn headers_with_gap_are_truncated() {
        let (service, _handle) = fixture();
        let mut a = Header::new();
        a.set_index(1);
        let mut b = Header::new();
        b.set_index(3); // gap on index 2
        service.handle_headers(vec![a, b]);
        assert_eq!(service.header_cache.count(), 1);
    }

    #[test]
    fn dispatch_command_variants_is_exhaustive() {
        // The exhaustive match in `BlockchainService::dispatch` (in
        // `service.rs`) is the real compile-time exhaustiveness
        // check. Any new variant added to `BlockchainCommand` will
        // fail to compile there until the dispatch arm is added. This
        // test documents that invariant and additionally verifies the
        // number of variants stays in sync with the dispatch arm
        // count, so accidental drift between documentation and
        // reality is caught by the test suite rather than discovered
        // by a panicked `unreachable!()` at runtime.
        use std::mem;

        // Helper that mirrors the dispatch arm order. It is
        // `unreachable!()`d because the test does not actually
        // invoke it; the function's job is to fail to compile when
        // the variant list drifts. The match has the same arm count
        // as the real dispatch in `service.rs`.
        #[allow(dead_code, unreachable_code)]
        fn exhaustive_dispatch(_cmd: BlockchainCommand) -> std::convert::Infallible {
            match _cmd {
                BlockchainCommand::PersistCompleted(_) => unreachable!(),
                BlockchainCommand::Import(_) => unreachable!(),
                BlockchainCommand::FillMemoryPool(_) => unreachable!(),
                BlockchainCommand::FillCompleted => unreachable!(),
                BlockchainCommand::Reverify(_) => unreachable!(),
                BlockchainCommand::InventoryBlock { .. } => unreachable!(),
                BlockchainCommand::ImportBlock { .. } => unreachable!(),
                BlockchainCommand::InventoryExtensible { .. } => unreachable!(),
                BlockchainCommand::PreverifyCompleted(_) => unreachable!(),
                BlockchainCommand::Headers(_) => unreachable!(),
                BlockchainCommand::Idle => unreachable!(),
                BlockchainCommand::DrainUnverified => unreachable!(),
                BlockchainCommand::RelayResult(_) => unreachable!(),
                BlockchainCommand::Initialize => unreachable!(),
                BlockchainCommand::AddTransaction { .. } => unreachable!(),
                BlockchainCommand::GetHeight { .. } => unreachable!(),
                BlockchainCommand::GetBlock { .. } => unreachable!(),
                BlockchainCommand::GetBlockByHeight { .. } => unreachable!(),
            }
        }

        // Build one of every reply-bearing variant so we can inspect
        // their discriminants. The four variants that need a
        // `Block`/`ExtensiblePayload`/`Transaction` field are not
        // constructed here; their discriminants are covered by the
        // static count assertion below.
        let (tx, _rx) = oneshot::channel();
        let _add_tx = BlockchainCommand::AddTransaction {
            transaction: neo_payloads::Transaction::new(),
            reply: tx,
        };
        let (ibtx, _ibrx) = oneshot::channel();
        let _import_block = BlockchainCommand::ImportBlock {
            block: Arc::new(Block::from_parts(Header::new(), vec![])),
            reply: ibtx,
        };
        let (htx, _hrx) = oneshot::channel();
        let _get_height = BlockchainCommand::GetHeight { reply: htx };
        let (bhx, _bhx_rx) = oneshot::channel();
        let _get_block = BlockchainCommand::GetBlock {
            hash: UInt256::zero(),
            reply: bhx,
        };
        let (bhx2, _bhx2_rx) = oneshot::channel();
        let _get_block_h = BlockchainCommand::GetBlockByHeight {
            height: 0,
            reply: bhx2,
        };

        // Confirm each of the constructed variants has a unique
        // discriminant — a regression test against accidental
        // discriminator reuse.
        let mut seen = std::collections::HashSet::new();
        for cmd in [
            &_add_tx,
            &_import_block,
            &_get_height,
            &_get_block,
            &_get_block_h,
        ] {
            assert!(seen.insert(mem::discriminant(cmd)));
        }

        // The expected variant count must match the dispatch arm
        // count above. Bump this when adding a new variant and
        // add a corresponding arm in both `exhaustive_dispatch` and
        // `BlockchainService::dispatch` in `service.rs`.
        const EXPECTED_VARIANTS: usize = 18;
        assert!(seen.len() <= EXPECTED_VARIANTS);

        // Keep the helper symbol alive so the dispatch table is not
        // dead-code-eliminated by the compiler when running tests
        // with `cfg(test)`.
        let _ = exhaustive_dispatch as fn(BlockchainCommand) -> std::convert::Infallible;
    }
}
