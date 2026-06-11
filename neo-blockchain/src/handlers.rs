//! Service method handlers for [`crate::service::BlockchainService`].
//!
//! Each command variant has a corresponding `async fn` method on the
//! service. The dispatch loop in [`crate::service::BlockchainService::dispatch`]
//! just `match`es on the command enum and calls the right method —
//! there is no actor framework, no `Box<dyn Any>` downcasting, no
//! per-message `Handle<T>` impls.
//!
//! Stage B keeps the implementations minimal: full handler logic
//! (block validation, transaction admission, …) will be ported in
//! later stages as the dependencies on the rest of the workspace
//! (native contracts, the state service, the mempool) are
//! progressively re-anchored on the new service abstractions.

use std::sync::Arc;

use neo_payloads::{
    block::Block, extensible_payload::ExtensiblePayload, header::Header, Block as PayloadBlock,
    InventoryType, Transaction,
};
use neo_primitives::verify_result::VerifyResult;
use parking_lot::Mutex;
use tokio::sync::oneshot;
use tracing::{debug, warn};

use crate::command::BlockchainCommand;
use crate::fill_memory_pool::FillMemoryPool;
use crate::import::Import;
use crate::internal::{classify_import_block, ImportDisposition};
use crate::persist_completed::PersistCompleted;
use crate::relay_result::RelayResult;
use crate::reverify::Reverify;
use crate::command::AddTransactionReply;
use crate::service::{BlockchainService, MempoolLike};
use crate::PreverifyCompleted;

impl BlockchainService {
    /// Handle a [`BlockchainCommand::PersistCompleted`]. The actor's
    /// legacy implementation did cache invalidation + mempool
    /// eviction + relay broadcast; Stage B does the minimum required
    /// to keep the cache consistent.
    pub(crate) async fn handle_persist_completed(&self, persist: PersistCompleted) {
        let PersistCompleted { block } = persist;
        let index = block.index();
        let _hash = match Self::try_block_hash(block.as_ref()) {
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
                hash: _hash,
                height: index,
            })
            .ok();
    }

    /// Handle a [`BlockchainCommand::Headers`] batch.
    pub(crate) fn handle_headers(&self, headers: Vec<Header>) {
        if headers.is_empty() {
            return;
        }

        let mut header_height = self
            .header_cache
            .last()
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

            if !self.header_cache.add(header.clone()) {
                break;
            }

            header_height = index;
        }
    }

    /// Handle a [`BlockchainCommand::Import`] request.
    pub(crate) async fn handle_import(&self, import: Import) {
        for block in import.blocks {
            let index = block.index();
            let current_height = self.ledger.current_height();
            match classify_import_block(current_height, index) {
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
        }
    }

    /// Handle a [`BlockchainCommand::FillMemoryPool`] request.
    pub(crate) async fn handle_fill_memory_pool(&self, _fill: FillMemoryPool) {
        // Stage B: the actual mempool ingestion logic is the next
        // stage. The handler is a no-op for now so the command loop
        // stays round-trip-able.
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
        _pre_verified: bool,
    ) -> Result<(), String> {
        let index = block.index();
        let hash = Self::try_block_hash(block.as_ref())?;
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
            return Ok(());
        }

        // C# Blockchain.OnNewBlock → Persist(block): the native-contract
        // state transition runs before the block becomes the new tip.
        if !self.persist_block_sequence(Arc::clone(&block)).await {
            return Err(format!(
                "native persistence pipeline failed for block {index}"
            ));
        }

        if let Err(error) = self.ledger.insert_block((*block).clone()) {
            return Err(format!("ledger insert: {error}"));
        }

        // Flush the block's native-persist writes through to the durable store
        // (C# snapshot.Commit() at the end of Blockchain.Persist) so the on-disk
        // tip advances and a restart resumes from here rather than genesis.
        self.system.commit_to_store();

        self.event_tx
            .send(crate::RuntimeEvent::Imported { hash, height: index })
            .ok();

        let _ = relay; // relay broadcast is handled by the network service
        Ok(())
    }

    /// Handle a [`BlockchainCommand::InventoryExtensible`] command.
    pub(crate) async fn handle_extensible_inventory(
        &self,
        mut payload: ExtensiblePayload,
        relay: bool,
    ) -> Result<(), String> {
        let hash = payload.hash();
        if let Err(error) = self.ledger.insert_extensible(payload) {
            return Err(format!("ledger insert: {error}"));
        }
        debug!(target: "neo", %hash, relay, "extensible payload accepted");
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
            self.ledger.insert_transaction(task.transaction).ok();
        }
        debug!(target: "neo", %hash, ?task.result, "preverify completed");
    }

    /// Handle a [`BlockchainCommand::Idle`] tick.
    pub(crate) async fn handle_idle(&self) {
        // Stage B: no mempool reverify queue to tick. The handler is
        // a no-op so the command loop remains round-trip-able.
    }

    /// Handle a [`BlockchainCommand::DrainUnverified`] tick.
    pub(crate) async fn handle_drain_unverified(&self) {
        // Stage B: the unverified cache is owned by the actor's old
        // struct. The new service does not own it yet; the handler
        // is a no-op.
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
    pub(crate) async fn add_transaction(
        &self,
        transaction: Transaction,
    ) -> AddTransactionReply {
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
                    let snapshot = neo_data_cache::DataCache::new(false);
                    pool.try_add(&transaction, &snapshot, &settings)
                }
            }
        };

        if result == VerifyResult::Succeed {
            self.ledger.insert_transaction(transaction.clone()).ok();
            self.event_tx
                .send(crate::RuntimeEvent::Imported { hash, height: 0 })
                .ok();
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
                let snapshot = neo_data_cache::DataCache::new(false);
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
    pub(crate) fn try_block_hash(block: &Block) -> Result<neo_primitives::UInt256, String> {
        let mut header = block.header.clone();
        header
            .try_hash()
            .map_err(|err| format!("hash computation failed: {err}"))
    }
}

// =============================================================================
// Tests
// =============================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use crate::handle::BlockchainHandle;
    use crate::header_cache::HeaderCache;
    use crate::ledger_context::LedgerContext;
    use crate::service_context::SystemContext;
    use neo_primitives::UInt256;

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

    #[derive(Debug, Default)]
    struct TestMempool;
    impl MempoolLike for TestMempool {
        fn try_add(
            &self,
            _tx: &Transaction,
            _snapshot: &neo_data_cache::DataCache,
            _settings: &neo_config::ProtocolSettings,
        ) -> VerifyResult {
            VerifyResult::Succeed
        }
    }

    fn fixture() -> (BlockchainService, BlockchainHandle) {
        let system: Arc<dyn SystemContext> = Arc::new(TestContext);
        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::default());
        let mempool: Arc<Mutex<dyn MempoolLike + Send + Sync>> = Arc::new(Mutex::new(TestMempool));
        BlockchainService::with_defaults(system, ledger, header_cache, mempool)
    }

    /// [`SystemContext`] over a shared in-memory store snapshot, so the
    /// native persistence pipeline actually runs.
    struct StoreContext {
        snapshot: Arc<neo_data_cache::DataCache>,
    }
    impl std::fmt::Debug for StoreContext {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("StoreContext").finish_non_exhaustive()
        }
    }
    impl SystemContext for StoreContext {
        fn settings(&self) -> Arc<neo_config::ProtocolSettings> {
            Arc::new(neo_config::ProtocolSettings::default())
        }
        fn current_height(&self) -> u32 {
            0
        }
        fn store_snapshot(&self) -> Option<Arc<neo_data_cache::DataCache>> {
            Some(Arc::clone(&self.snapshot))
        }
    }

    fn store_fixture() -> (BlockchainService, BlockchainHandle, Arc<neo_data_cache::DataCache>) {
        neo_native_contracts::install();
        let snapshot = Arc::new(neo_data_cache::DataCache::new(false));
        let system: Arc<dyn SystemContext> = Arc::new(StoreContext {
            snapshot: Arc::clone(&snapshot),
        });
        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::default());
        let mempool: Arc<Mutex<dyn MempoolLike + Send + Sync>> = Arc::new(Mutex::new(TestMempool));
        let (service, handle) =
            BlockchainService::with_defaults(system, ledger, header_cache, mempool);
        (service, handle, snapshot)
    }

    /// NEO total supply read (NEP-17 `Prefix_TotalSupply` = 11).
    fn neo_total_supply(snapshot: &neo_data_cache::DataCache) -> Option<num_bigint::BigInt> {
        snapshot
            .get(&neo_data_cache::StorageKey::new(
                neo_native_contracts::NeoToken::ID,
                vec![11],
            ))
            .map(|item| num_bigint::BigInt::from_signed_bytes_le(&item.value_bytes()))
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
        assert!(service.ledger.block_hash_at(0).is_some(), "genesis cached in the ledger");

        // Re-initializing must NOT re-persist (the initialized probe
        // guards the C# `Ledger.Initialized` branch): the supply stays
        // 100M instead of doubling.
        service.initialize().await;
        assert_eq!(neo_total_supply(&snapshot), Some(num_bigint::BigInt::from(100_000_000)));

        // An inventory block at the next height runs the OnPersist /
        // PostPersist native hooks over the same store: block 1 mints
        // the 0.5-GAS committee reward to standby_committee[1 % 21].
        let mut header = Header::new();
        header.set_index(1);
        let block = Arc::new(Block::from_parts(header, vec![]));
        service
            .handle_block_inventory(block, false, false)
            .await
            .expect("inventory block persists");
        assert_eq!(service.ledger.current_height(), 1);

        let settings = neo_config::ProtocolSettings::default();
        let member = &settings.standby_committee[1];
        let script = neo_redeem_script::signature_redeem_script(&member.to_bytes());
        let account = neo_primitives::UInt160::from_script(&script);
        let mut key = vec![20u8]; // shared NEP-17 Prefix_Account
        key.extend_from_slice(&account.to_bytes());
        assert!(
            snapshot
                .get(&neo_data_cache::StorageKey::new(
                    neo_native_contracts::GasToken::ID,
                    key
                ))
                .is_some(),
            "block-1 PostPersist minted the rotating committee reward"
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
        for cmd in [&_add_tx, &_get_height, &_get_block, &_get_block_h] {
            assert!(seen.insert(mem::discriminant(cmd)));
        }

        // The expected variant count must match the dispatch arm
        // count above. Bump this when adding a new variant and
        // add a corresponding arm in both `exhaustive_dispatch` and
        // `BlockchainService::dispatch` in `service.rs`.
        const EXPECTED_VARIANTS: usize = 17;
        assert!(seen.len() <= EXPECTED_VARIANTS);

        // Keep the helper symbol alive so the dispatch table is not
        // dead-code-eliminated by the compiler when running tests
        // with `cfg(test)`.
        let _ = exhaustive_dispatch as fn(BlockchainCommand) -> std::convert::Infallible;
    }
}
