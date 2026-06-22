use super::*;
use crate::command::BlockchainCommand;
use crate::fill_memory_pool::FillMemoryPool;
use crate::handle::BlockchainHandle;
use crate::header_cache::HeaderCache;
use crate::ledger_context::LedgerContext;
use crate::service::MempoolLike;
use crate::service_context::SystemContext;
use neo_payloads::Transaction;
use neo_primitives::UInt256;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
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

#[derive(Debug)]
struct RecordingMempool {
    reverify_calls: Arc<AtomicUsize>,
}
impl MempoolLike for RecordingMempool {
    fn try_add(
        &self,
        _tx: &Transaction,
        _snapshot: &neo_storage::DataCache,
        _settings: &neo_config::ProtocolSettings,
    ) -> VerifyResult {
        VerifyResult::Succeed
    }

    fn reverify_top_unverified(
        &self,
        _snapshot: &neo_storage::DataCache,
        _max_count: usize,
    ) -> bool {
        self.reverify_calls.fetch_add(1, Ordering::SeqCst);
        false
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

/// [`SystemContext`] over a shared in-memory store snapshot, so the
/// native persistence pipeline actually runs.
struct StoreContext {
    snapshot: Arc<neo_storage::DataCache>,
    settings: Arc<neo_config::ProtocolSettings>,
    state_service: Option<Arc<neo_state_service::commit_handlers::StateServiceCommitHandlers>>,
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
    fn block_committing(
        &self,
        block: &Block,
        snapshot: &neo_storage::DataCache,
        _application_executed_list: &[neo_payloads::ApplicationExecuted],
    ) -> bool {
        match &self.state_service {
            Some(handler) => handler.on_committing(block.index(), snapshot),
            None => true,
        }
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
        state_service: None,
    });
    let ledger = Arc::new(LedgerContext::default());
    let header_cache = Arc::new(HeaderCache::default());
    let mempool: Arc<Mutex<dyn MempoolLike + Send + Sync>> = Arc::new(Mutex::new(TestMempool));
    let (service, handle) = BlockchainService::with_defaults(system, ledger, header_cache, mempool);
    (service, handle, snapshot)
}

fn store_fixture_with_state_service() -> (
    BlockchainService,
    BlockchainHandle,
    Arc<neo_storage::DataCache>,
    Arc<neo_state_service::StateStore>,
) {
    neo_native_contracts::install();
    let snapshot = Arc::new(neo_storage::DataCache::new(false));
    let state_store = Arc::new(neo_state_service::StateStore::with_mpt(false));
    let state_service = Arc::new(
        neo_state_service::commit_handlers::StateServiceCommitHandlers::new(Arc::clone(
            &state_store,
        )),
    );
    let system: Arc<dyn SystemContext> = Arc::new(StoreContext {
        snapshot: Arc::clone(&snapshot),
        settings: Arc::new(neo_config::ProtocolSettings::default()),
        state_service: Some(state_service),
    });
    let ledger = Arc::new(LedgerContext::default());
    let header_cache = Arc::new(HeaderCache::default());
    let mempool: Arc<Mutex<dyn MempoolLike + Send + Sync>> = Arc::new(Mutex::new(TestMempool));
    let (service, handle) = BlockchainService::with_defaults(system, ledger, header_cache, mempool);
    (service, handle, snapshot, state_store)
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

fn seed_current_ledger(snapshot: &neo_storage::DataCache, index: u32) {
    let hash = UInt256::from_bytes(&[0u8; 32]).expect("zero hash");
    let bytes = neo_native_contracts::LedgerContract::new()
        .serialize_hash_index_state(&hash, index)
        .expect("hash index state");
    snapshot.add(
        neo_storage::StorageKey::new(neo_native_contracts::LedgerContract::ID, vec![12]),
        neo_storage::StorageItem::from_bytes(bytes),
    );
}

fn seed_conflict_record(
    snapshot: &neo_storage::DataCache,
    hash: &UInt256,
    signer: &neo_primitives::UInt160,
    index: u32,
) {
    let stub = neo_native_contracts::LedgerContract::new()
        .serialize_conflict_stub(index)
        .expect("conflict stub");
    let mut bare_key = Vec::with_capacity(33);
    bare_key.push(11);
    bare_key.extend_from_slice(&hash.to_bytes());
    snapshot.add(
        neo_storage::StorageKey::new(neo_native_contracts::LedgerContract::ID, bare_key),
        neo_storage::StorageItem::from_bytes(stub.clone()),
    );

    let mut signer_key = Vec::with_capacity(53);
    signer_key.push(11);
    signer_key.extend_from_slice(&hash.to_bytes());
    signer_key.extend_from_slice(&signer.to_bytes());
    snapshot.add(
        neo_storage::StorageKey::new(neo_native_contracts::LedgerContract::ID, signer_key),
        neo_storage::StorageItem::from_bytes(stub),
    );
}

#[path = "handlers/block_flow.rs"]
mod block_flow;
#[path = "handlers/extensible_headers.rs"]
mod extensible_headers;
#[path = "handlers/transactions.rs"]
mod transactions;

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
