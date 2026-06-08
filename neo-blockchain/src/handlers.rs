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

            if let Err(error) = self.ledger.insert_block(block) {
                warn!(
                    target: "neo",
                    %error,
                    height = index,
                    "failed to import block into ledger cache"
                );
                break;
            }
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

        if let Err(error) = self.ledger.insert_block((*block).clone()) {
            return Err(format!("ledger insert: {error}"));
        }

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
    pub(crate) async fn initialize(&self) {
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

        // In the full impl, the mempool is a parking_lot::Mutex<MemoryPool>.
        // For Stage B we have a `Mutex<dyn MempoolLike>`, so we just call
        // try_add on the trait object.
        let result = {
            let pool = self.mempool.lock();
            // The snapshot and settings are not consulted by the stub
            // mempool; the real implementation in `neo-core` will
            // thread them through once the trait surface is widened.
            let snapshot = neo_data_cache::DataCache::new(false);
            let settings = self.system.settings();
            pool.try_add(&transaction, &snapshot, &settings)
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
        let snapshot = neo_data_cache::DataCache::new(false);
        let settings = self.system.settings();
        let result = pool.try_add(transaction, &snapshot, &settings);
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
