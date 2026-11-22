use super::*;
use neo_ledger::{HeaderCache, LedgerContext};
use neo_native_contracts::ledger_contract::LedgerContract;
use neo_native_contracts::native::NativeHelpers;
use neo_native_contracts::native::composition::default_native_services;
use neo_native_contracts::{ContractManagement, GasToken};
use neo_primitives::{UInt160, WitnessScope};
use neo_protocol::ProtocolSettings;
use neo_protocol::payloads::{Header, Signer, Transaction};
use neo_protocol::witness::Witness;
use neo_smart_contract::application_engine::NativeContract;
use neo_storage::error::{StorageError, StorageResult};
use neo_storage::persistence::providers::memory_store::MemoryStore;
use neo_storage::persistence::{
    ReadOnlyStore, ReadOnlyStoreGeneric, SeekDirection, StoreSnapshot, WriteStore,
    store::OnNewSnapshotDelegate, store_provider::StoreProvider,
};
use neo_storage::{StorageItem, StorageKey};
use neo_vm::{OpCode, ScriptBuilder};
use num_bigint::BigInt;
use std::any::Any;

fn transfer_block(
    system: &NeoSystem,
    from: UInt160,
    recipient: UInt160,
    amount: i64,
    witness: UInt160,
) -> neo_protocol::payloads::Block {
    let mut builder = ScriptBuilder::new();
    builder.emit_opcode(OpCode::PUSHNULL);
    builder.emit_push_int(amount);
    builder.emit_push(&recipient.to_bytes());
    builder.emit_push(&from.to_bytes());
    builder.emit_push_int(4);
    builder.emit_opcode(OpCode::PACK);
    builder.emit_push_int(CallFlags::ALL.bits() as i64);
    builder.emit_push(b"transfer");
    builder.emit_push(&GasToken::new().hash().to_bytes());
    builder
        .emit_syscall("System.Contract.Call")
        .expect("syscall");
    builder.emit_opcode(OpCode::ASSERT);

    let mut transaction = Transaction::new();
    transaction.set_nonce(1);
    transaction.set_system_fee(TEST_MODE_GAS);
    transaction.set_network_fee(0);
    transaction.set_valid_until_block(100);
    transaction.set_signers(vec![Signer::new(witness, WitnessScope::CALLED_BY_ENTRY)]);
    transaction.set_witnesses(vec![Witness::new()]);
    transaction.set_script(builder.to_array());

    let mut header = Header::new();
    header.set_index(1);
    header.set_prev_hash(system.genesis_block().hash());
    header.set_next_consensus(UInt160::zero());
    header.set_timestamp(1);
    header.witness = Witness::new();
    let mut block = neo_protocol::payloads::Block {
        header,
        transactions: vec![transaction],
    };
    block.rebuild_merkle_root();
    block
}

/// A memory store whose snapshots can fail the physical commit on demand.
///
/// Clones share the underlying map and the failure switch, so every transaction
/// opened through this store observes the same commit behaviour.
#[derive(Clone)]
struct FlakyStore {
    inner: MemoryStore,
    fail_commits: Arc<AtomicBool>,
}

struct FlakyStoreProvider {
    store: FlakyStore,
}

impl StoreProvider for FlakyStoreProvider {
    fn name(&self) -> &str {
        "Flaky"
    }

    fn get_store(&self, _path: &str) -> StorageResult<Arc<dyn Store>> {
        Ok(Arc::new(self.store.clone()))
    }
}

impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for FlakyStore {
    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        self.inner.try_get(key)
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        self.inner.find(key_prefix, direction)
    }
}

impl ReadOnlyStore for FlakyStore {}

impl WriteStore<Vec<u8>, Vec<u8>> for FlakyStore {
    fn delete(&mut self, key: Vec<u8>) -> StorageResult<()> {
        self.inner.delete(key)
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> StorageResult<()> {
        self.inner.put(key, value)
    }
}

impl Store for FlakyStore {
    fn snapshot(&self) -> Arc<dyn StoreSnapshot> {
        Arc::new(FlakySnapshot {
            inner: self.inner.snapshot(),
            store: self.clone(),
        })
    }

    fn on_new_snapshot(&self, _handler: OnNewSnapshotDelegate) {}

    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct FlakySnapshot {
    inner: Arc<dyn StoreSnapshot>,
    store: FlakyStore,
}

impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for FlakySnapshot {
    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        self.inner.try_get(key)
    }

    fn find(
        &self,
        key_prefix: Option<&Vec<u8>>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        self.inner.find(key_prefix, direction)
    }
}

impl WriteStore<Vec<u8>, Vec<u8>> for FlakySnapshot {
    fn delete(&mut self, key: Vec<u8>) -> StorageResult<()> {
        Arc::get_mut(&mut self.inner)
            .ok_or_else(shared_snapshot_error)?
            .delete(key)
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> StorageResult<()> {
        Arc::get_mut(&mut self.inner)
            .ok_or_else(shared_snapshot_error)?
            .put(key, value)
    }
}

/// Each test snapshot is created and consumed by a single writer; a shared
/// handle means the fixture was misused, so fail loudly instead.
fn shared_snapshot_error() -> StorageError {
    StorageError::InvalidOperation {
        message: "flaky test snapshot is shared".to_string(),
    }
}

impl StoreSnapshot for FlakySnapshot {
    fn store(&self) -> Arc<dyn Store> {
        Arc::new(self.store.clone())
    }

    fn try_commit(&mut self) -> Result<(), StorageError> {
        if self.store.fail_commits.load(Ordering::SeqCst) {
            return Err(StorageError::InvalidOperation {
                message: "injected physical commit failure".to_string(),
            });
        }
        Arc::get_mut(&mut self.inner)
            .ok_or_else(shared_snapshot_error)?
            .try_commit()
    }
}

fn execution_observation(executions: &[ApplicationExecuted]) -> Vec<String> {
    executions
        .iter()
        .map(|execution| {
            format!(
                "{:?}|{:?}|{}|{:?}|{:?}|{:?}|{:?}",
                execution.trigger,
                execution.vm_state,
                execution.gas_consumed,
                execution.exception,
                execution.stack,
                execution.notifications,
                execution.logs
            )
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn block_level_transfer_modes_match_and_faults_roll_back() {
    let settings = ProtocolSettings::testnet();
    let sender = NativeHelpers::get_bft_address(&settings.standby_validators());
    let recipient = UInt160::from_bytes(&[0x42; 20]).expect("recipient");
    let stranger = UInt160::from_bytes(&[0x24; 20]).expect("stranger");
    let mut expected_success = None;
    let mut expected_faults = None;

    for mode in [
        ExecutionMode::Canonical,
        ExecutionMode::Auto,
        ExecutionMode::Verify,
    ] {
        let system = NeoSystem::new(settings.clone(), None, None).expect("system");
        let block = transfer_block(&system, sender, recipient, 1_000, sender);
        let before = GasToken::new().balance_of_snapshot(&system.store_cache(), &sender);
        let executions = system
            .persist_block_with_receipts_and_execution_mode(Block::from(&block), mode)
            .expect("successful transfer");
        let success = execution_observation(&executions);
        assert_eq!(executions[1].vm_state, VmState::HALT);
        assert_eq!(
            GasToken::new().balance_of_snapshot(&system.store_cache(), &recipient),
            BigInt::from(1_000)
        );
        assert_eq!(
            before - GasToken::new().balance_of_snapshot(&system.store_cache(), &sender),
            BigInt::from(TEST_MODE_GAS + 1_000)
        );
        if let Some(expected) = &expected_success {
            assert_eq!(expected, &success, "success differs in {}", mode.as_str());
        } else {
            expected_success = Some(success);
        }
        // The direct-dispatch counter now crosses the strategy port, so a
        // miswired observer would silently stop reporting here.
        match mode {
            ExecutionMode::Canonical => assert_eq!(
                system.execution_stats().direct_transfer_count(),
                0,
                "canonical mode must not report a direct dispatch"
            ),
            ExecutionMode::Auto | ExecutionMode::Verify => assert!(
                system.execution_stats().direct_transfer_count() > 0,
                "{} must report a direct dispatch",
                mode.as_str()
            ),
        }

        let sender_after_success =
            GasToken::new().balance_of_snapshot(&system.store_cache(), &sender);
        let mut fault_observations = Vec::new();
        let mut previous_hash = block.hash().expect("block hash");
        for (offset, (from, amount)) in [(sender, i64::MAX), (stranger, 1)].into_iter().enumerate()
        {
            // The fee payer remains funded; the second case faults because
            // the transfer source is not a transaction witness.
            let mut fault = transfer_block(&system, from, recipient, amount, sender);
            fault.header.set_index(2 + offset as u32);
            fault.header.set_prev_hash(previous_hash);
            fault.rebuild_merkle_root();
            previous_hash = fault.hash().expect("fault block hash");
            let executions = system
                .persist_block_with_receipts_and_execution_mode(Block::from(&fault), mode)
                .expect("faulting transactions are persisted with rollback");
            assert_eq!(executions[1].vm_state, VmState::FAULT);
            fault_observations.push(execution_observation(&executions));
        }
        // Fault rolls back token movements, not the canonical block fee burn.
        assert_eq!(
            sender_after_success
                - GasToken::new().balance_of_snapshot(&system.store_cache(), &sender),
            BigInt::from(2 * TEST_MODE_GAS)
        );
        assert_eq!(
            GasToken::new().balance_of_snapshot(&system.store_cache(), &recipient),
            BigInt::from(1_000)
        );
        if let Some(expected) = &expected_faults {
            assert_eq!(
                expected,
                &fault_observations,
                "fault differs in {}",
                mode.as_str()
            );
        } else {
            expected_faults = Some(fault_observations);
        }
        system.shutdown().await.expect("shutdown");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn consecutive_empty_blocks_keep_native_pipeline_across_committee_refresh() {
    let settings = ProtocolSettings::testnet();
    let block_count = 2 * settings.committee_members_count() as u32 + 2;
    let system = NeoSystem::new(settings, None, None).expect("system");
    let mut previous_hash = system.genesis_block().hash();

    for index in 1..=block_count {
        let mut header = Header::new();
        header.set_index(index);
        header.set_prev_hash(previous_hash);
        header.set_next_consensus(UInt160::zero());
        header.set_timestamp(u64::from(index));
        header.witness = Witness::new();
        let payload = neo_protocol::payloads::Block {
            header,
            transactions: Vec::new(),
        };
        previous_hash = payload.hash().expect("empty block hash");
        let executions = system
            .persist_block_with_receipts_and_execution_mode(
                Block::from(&payload),
                ExecutionMode::Auto,
            )
            .expect("empty block");
        assert_eq!(executions.len(), 2, "OnPersist/PostPersist must both run");
        assert!(
            executions
                .iter()
                .all(|execution| execution.vm_state == VmState::HALT)
        );
        assert_eq!(system.current_block_index(), index);
    }
    system.shutdown().await.expect("shutdown");
}

/// Builds an empty single-witness block on top of `prev_hash`.
fn empty_block_at(index: u32, prev_hash: neo_primitives::UInt256) -> neo_protocol::payloads::Block {
    let mut header = Header::new();
    header.set_index(index);
    header.set_prev_hash(prev_hash);
    header.set_next_consensus(UInt160::zero());
    header.set_timestamp(u64::from(index));
    header.witness = Witness::new();
    neo_protocol::payloads::Block {
        header,
        transactions: Vec::new(),
    }
}

/// State-service settings that persist a state root for every height.
fn state_service_settings(
    settings: &ProtocolSettings,
) -> crate::state_service::StateServiceSettings {
    crate::state_service::StateServiceSettings {
        full_state: false,
        path: String::new(),
        network: settings.network,
        auto_verify: false,
        max_find_result_items: 100,
        exception_policy: crate::state_service::UnhandledExceptionPolicy::Ignore,
    }
}

/// Empty-block differential across execution engines: for one identical empty
/// block, every mode must produce the same receipts, ledger pointer and state
/// root. This pins the canonical empty-block outcome the hook fast paths must
/// keep reproducing.
#[tokio::test(flavor = "multi_thread")]
async fn empty_block_receipts_roots_and_pointers_match_across_execution_modes() {
    let settings = ProtocolSettings::testnet();
    let mut expected: Option<(
        Vec<String>,
        neo_primitives::UInt256,
        neo_primitives::UInt256,
    )> = None;

    for mode in [
        ExecutionMode::Canonical,
        ExecutionMode::Auto,
        ExecutionMode::Verify,
    ] {
        let system = NeoSystem::new_with_state_service(
            settings.clone(),
            None,
            None,
            Some(state_service_settings(&settings)),
        )
        .expect("system");
        let block = empty_block_at(1, system.genesis_block().hash());
        let block_hash = block.hash().expect("empty block hash");

        let executions = system
            .persist_block_with_receipts_and_execution_mode(Block::from(&block), mode)
            .expect("empty block");

        assert_eq!(
            executions.len(),
            2,
            "OnPersist/PostPersist must both run in {}",
            mode.as_str()
        );
        assert!(
            executions
                .iter()
                .all(|execution| execution.vm_state == VmState::HALT),
            "empty block faulted in {}",
            mode.as_str()
        );
        assert!(
            executions[0].notifications.is_empty(),
            "an empty OnPersist must not notify in {}",
            mode.as_str()
        );

        let ledger = LedgerContract::new();
        let store_cache = system.store_cache();
        assert_eq!(ledger.current_index(&store_cache).expect("ledger index"), 1);
        assert_eq!(
            ledger.current_hash(&store_cache).expect("ledger hash"),
            block_hash
        );

        let handlers = system.state_commit_handlers().expect("state handlers");
        let root = handlers
            .get_state_root(1)
            .expect("a state root record per height");
        assert_eq!(root.index, 1);
        assert_eq!(
            handlers.current_root_hash(),
            Some(root.root_hash),
            "process-local root must match the persisted root in {}",
            mode.as_str()
        );

        let observation = (
            execution_observation(&executions),
            block_hash,
            root.root_hash,
        );
        match &expected {
            Some(expected) => assert_eq!(
                expected,
                &observation,
                "empty block differs in {}",
                mode.as_str()
            ),
            None => expected = Some(observation),
        }

        system.shutdown().await.expect("shutdown");
    }
}

/// Consecutive-empty-block regression: the optimized hooks must not remove any
/// per-height transition. Every empty block still runs both persist triggers,
/// advances the ledger pointer, persists a state-root record, and NEO still
/// refreshes the committee at the committee-size boundary.
#[tokio::test(flavor = "multi_thread")]
async fn consecutive_empty_blocks_keep_height_dependent_transitions() {
    let settings = ProtocolSettings::testnet();
    let committee = u32::try_from(settings.committee_members_count()).expect("committee size");
    let block_count = 2 * committee + 1;
    let system = NeoSystem::new_with_state_service(
        settings.clone(),
        None,
        None,
        Some(state_service_settings(&settings)),
    )
    .expect("system");
    let handlers = system.state_commit_handlers().expect("state handlers");
    let neo = neo_native_contracts::NeoToken::new();
    let mut previous_hash = system.genesis_block().hash();
    let mut refreshed_committee: Option<Vec<Vec<u8>>> = None;
    let mut refresh_heights = Vec::new();

    for index in 1..=block_count {
        let payload = empty_block_at(index, previous_hash);
        previous_hash = payload.hash().expect("empty block hash");
        let executions = system
            .persist_block_with_receipts_and_execution_mode(
                Block::from(&payload),
                ExecutionMode::Auto,
            )
            .expect("empty block");

        assert_eq!(
            executions.len(),
            2,
            "OnPersist/PostPersist must both run at height {index}"
        );
        assert!(
            executions
                .iter()
                .all(|execution| execution.vm_state == VmState::HALT),
            "empty block faulted at height {index}"
        );
        // No fee burn, no notary or oracle reward, no token transfer: an
        // empty block's OnPersist emits no notification.
        assert!(
            executions[0].notifications.is_empty(),
            "empty OnPersist emitted a notification at height {index}"
        );

        // Ledger pointer transition for every height.
        assert_eq!(system.current_block_index(), index);
        let ledger = LedgerContract::new();
        let store_cache = system.store_cache();
        assert_eq!(
            ledger.current_index(&store_cache).expect("ledger index"),
            index,
            "ledger index must follow the tip at height {index}"
        );
        assert_eq!(
            ledger.current_hash(&store_cache).expect("ledger hash"),
            previous_hash,
            "ledger hash must follow the tip at height {index}"
        );

        // A state-root record is persisted for every height, even when the
        // root value is unchanged because the block had no transactions.
        let root = handlers
            .get_state_root(index)
            .unwrap_or_else(|| panic!("no state root record at height {index}"));
        assert_eq!(root.index, index);
        assert_eq!(handlers.current_root_hash(), Some(root.root_hash));

        // NEO committee state across the committee-size boundary. Genesis
        // initialises it and every refresh height rewrites it, so on an empty
        // chain it must stay present, non-empty and drift-free.
        let committee_now = neo
            .committee_from_snapshot(&store_cache)
            .map(|keys| {
                keys.iter()
                    .map(|key| key.as_bytes().to_vec())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| panic!("committee missing at height {index}"));
        assert!(
            !committee_now.is_empty(),
            "committee empty at height {index}"
        );
        match &refreshed_committee {
            Some(expected) => assert_eq!(
                expected, &committee_now,
                "committee drifted at height {index}"
            ),
            None => refreshed_committee = Some(committee_now),
        }
        if index % committee == 0 {
            refresh_heights.push(index);
        }
    }

    assert_eq!(
        refresh_heights,
        vec![committee, 2 * committee],
        "committee refresh heights must stay on the committee-size boundary"
    );
    assert_eq!(system.current_block_index(), block_count);
    system.shutdown().await.expect("shutdown");
}

#[test]
fn execution_recorder_is_empty_before_capture() {
    let recorder = ExecutionRecorder::default();
    assert!(recorder.snapshot().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn empty_block_batch_physical_commit_failure_poisons_system() {
    let settings = ProtocolSettings::default();
    let fail_commits = Arc::new(AtomicBool::new(false));
    let provider: Arc<dyn StoreProvider> = Arc::new(FlakyStoreProvider {
        store: FlakyStore {
            inner: MemoryStore::new(),
            fail_commits: Arc::clone(&fail_commits),
        },
    });
    // Genesis is persisted while the store is healthy, so only the batch's
    // single physical commit fails.
    let system = NeoSystem::new(settings, Some(provider), None).expect("system");
    assert!(!system.is_persistence_poisoned());

    let mut blocks = Vec::new();
    let mut previous_hash = system.genesis_block().hash();
    for index in 1..=2u32 {
        let mut header = Header::new();
        header.set_index(index);
        header.set_prev_hash(previous_hash);
        header.set_next_consensus(UInt160::zero());
        header.set_timestamp(u64::from(index));
        header.witness = Witness::new();
        let payload = neo_protocol::payloads::Block {
            header,
            transactions: Vec::new(),
        };
        previous_hash = payload.hash().expect("empty block hash");
        blocks.push(payload);
    }

    fail_commits.store(true, Ordering::SeqCst);
    let error =
        expect_err(system.persist_consecutive_empty_blocks_without_runtime_cache(blocks.clone()));
    assert!(
        error.contains("injected physical commit failure"),
        "unexpected error: {error}"
    );
    assert!(
        system.is_persistence_poisoned(),
        "a failed batch commit must poison the system"
    );

    let error = expect_err(system.persist_block(Block::from(&blocks[0])));
    assert!(
        error.to_lowercase().contains("poisoned"),
        "unexpected error: {error}"
    );
    let error = expect_err(system.persist_consecutive_empty_blocks_without_runtime_cache(blocks));
    assert!(
        error.to_lowercase().contains("poisoned"),
        "unexpected error: {error}"
    );
    system.shutdown().await.expect("shutdown");
}

/// Unwraps a persistence error without requiring `Debug` on the success value.
fn expect_err<T>(result: Result<T, String>) -> String {
    match result {
        Ok(_) => panic!("expected a persistence error"),
        Err(error) => error,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn persisted_store_rehydrates_ledger_tip_and_native_contract_cache() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
    let genesis_hash = system.genesis_block().hash();
    let store_cache = system.store_cache();

    // Use a fresh registry to prove that hydration restores persisted native
    // metadata rather than observing cache entries created during execution.
    let native_services = default_native_services();
    let management = native_services
        .registry
        .get(&ContractManagement::contract_hash())
        .expect("ContractManagement in registry");
    let management = management
        .as_any()
        .downcast_ref::<ContractManagement>()
        .expect("concrete ContractManagement");
    let ledger_hash = LedgerContract::new().hash();
    assert!(management.get_contract(&ledger_hash).unwrap().is_none());
    crate::neo_system::storage::hydrate_native_contracts(
        &store_cache,
        &native_services,
        system.settings(),
    )
    .expect("hydrate nonempty native storage");
    assert!(management.get_contract(&ledger_hash).unwrap().is_some());

    let mut header = Header::new();
    header.set_index(1);
    header.set_prev_hash(genesis_hash);
    header.set_next_consensus(UInt160::zero());
    header.set_timestamp(1);
    header.witness = Witness::new();
    let payload = neo_protocol::payloads::Block {
        header,
        transactions: Vec::new(),
    };
    let block = Block::from(&payload);
    let block_hash = block.hash();
    system.persist_block(block).expect("persist block");

    let store_cache = system.store_cache();
    let ledger = Arc::new(LedgerContext::default());
    let header_cache = Arc::new(HeaderCache::new());
    neo_ledger::hydrate_ledger(&store_cache, &ledger, &header_cache)
        .expect("rehydrate persisted chain");
    assert_eq!(ledger.current_height(), 1);
    assert_eq!(ledger.block_hash_at(0), Some(genesis_hash));
    assert_eq!(ledger.block_hash_at(1), Some(block_hash));
    assert_eq!(header_cache.count(), 2);
    system.shutdown().await.expect("shutdown");
}

#[test]
fn fast_path_counters_classify_each_outcome_once() {
    let stats = ExecutionStats::default();

    stats.record_strategy_outcome(ExecutionStrategy::DirectTransfer, VmState::HALT, None);
    stats.record_strategy_outcome(ExecutionStrategy::DirectTransfer, VmState::FAULT, None);
    stats.record_strategy_outcome(
        ExecutionStrategy::DirectTransfer,
        VmState::HALT,
        Some("direct fingerprint diverged"),
    );
    stats.record_strategy_outcome(
        ExecutionStrategy::Canonical,
        VmState::HALT,
        Some("missing isolated verification context"),
    );
    // A canonical run without a recorded reason is the configured canonical
    // mode, not a fallback.
    stats.record_strategy_outcome(ExecutionStrategy::Canonical, VmState::HALT, None);

    let snapshot = stats.fast_path_snapshot();
    assert_eq!(snapshot.direct_halts, 2);
    assert_eq!(snapshot.direct_faults, 1);
    assert_eq!(snapshot.canonical_fallbacks, 1);
    assert_eq!(snapshot.verification_mismatches, 1);
    // Outcome classification never invents direct attempts; the strategy
    // layer reports those before the result is known.
    assert_eq!(snapshot.direct_attempts, 0);
}

#[test]
fn fast_path_declines_are_counted_from_the_observer_port() {
    let stats = ExecutionStats::default();

    stats.record_fast_path_decline();
    stats.record_fast_path_decline();
    stats.record_direct_transfer_attempt();

    let snapshot = stats.fast_path_snapshot();
    assert_eq!(snapshot.eligibility_declines, 2);
    assert_eq!(snapshot.direct_attempts, 1);
    assert_eq!(stats.direct_transfer_count(), 1);
}

#[test]
fn profiling_disabled_keeps_mpt_timings_at_zero() {
    let stats = ExecutionStats::default();
    stats.record_state_root_profile(
        Duration::from_millis(10),
        Duration::from_millis(20),
        MptProfileSnapshot {
            update: Duration::from_millis(5),
            hash: Duration::from_millis(2),
            ..MptProfileSnapshot::default()
        },
    );

    let profile = stats.profile_snapshot();
    assert!(!profile.enabled);
    assert_eq!(profile.mpt, Duration::ZERO);
    assert_eq!(profile.mpt_update, Duration::ZERO);

    stats.set_profiling(true);
    stats.record_state_root_profile(
        Duration::from_millis(10),
        Duration::from_millis(20),
        MptProfileSnapshot {
            update: Duration::from_millis(5),
            hash: Duration::from_millis(2),
            ..MptProfileSnapshot::default()
        },
    );

    let profile = stats.profile_snapshot();
    assert!(profile.enabled);
    assert_eq!(profile.mpt, Duration::from_millis(20));
    assert_eq!(profile.mpt_update, Duration::from_millis(5));
    assert_eq!(profile.mpt_hash, Duration::from_millis(2));
}

#[test]
fn state_root_profile_sink_follows_the_profiling_switch() {
    let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
    let cache = Arc::new(neo_storage::persistence::StoreCache::new(store));
    let handlers = crate::state_service::StateServiceCommitHandlers::new(cache, false)
        .expect("state handlers");
    let stats = Arc::new(ExecutionStats::default());

    assert!(!handlers.profile_sink_registered());

    install_state_root_profile_sink(&handlers, &stats, true);
    assert!(handlers.profile_sink_registered());

    install_state_root_profile_sink(&handlers, &stats, false);
    assert!(!handlers.profile_sink_registered());
}

#[tokio::test(flavor = "multi_thread")]
async fn execution_profiling_switch_is_observation_only() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
    // This instance runs without a state service, so the switch only flips
    // the profiling gate and must never touch an execution result.
    assert!(system.state_commit_handlers().is_none());
    assert!(!system.execution_stats().profile_snapshot().enabled);

    system.set_execution_profiling(true);
    assert!(system.execution_stats().profile_snapshot().enabled);
    assert!(system.state_commit_handlers().is_none());

    system.set_execution_profiling(false);
    assert!(!system.execution_stats().profile_snapshot().enabled);

    system.shutdown().await.expect("shutdown");
}

#[test]
fn persistence_stage_nesting_names_every_overlapping_stage() {
    // Consumers such as the offline `.acc` import summary publish each stage
    // as its own measurement, so the rule that overlapping stages are never
    // summed has to stay discoverable at the definition site.
    for relation in [
        "transaction_vm<transaction_loop",
        "state_change_extraction+mpt<participant_precommit",
        "store_database_write<store_commit",
    ] {
        assert!(
            PERSISTENCE_STAGE_NESTING.contains(relation),
            "PERSISTENCE_STAGE_NESTING lost the {relation} relation"
        );
    }
}
