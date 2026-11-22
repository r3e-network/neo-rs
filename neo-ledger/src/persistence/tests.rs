//! Operation-level tests for the pure [`persist_block`] pipeline.
//!
//! These exercise the canonical 15-step sequencing, the HALT/FAULT/other VM-state
//! disposition (steps 10a/10b/10c), reverse-order participant discard (steps 12/13/14),
//! the store-commit and runtime-cache error `Display` literals, unconditional VM-state
//! recording under suppressed receipts, and bounded post-commit evidence export
//! (step 15b). The six verbatim fast-sync integration tests remain owned by the
//! `neo-system` adapter (task-102 Part I) and are *not* duplicated here.

use super::*;
use neo_primitives::{TriggerType, UInt256, Verifiable};
use neo_protocol::payloads::Transaction;
use neo_smart_contract::application_engine::{LedgerTransactionStates, NativeContractsCache};
use neo_vm::VmState;
use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};

type Log = Arc<Mutex<Vec<String>>>;

fn log() -> Log {
    Arc::new(Mutex::new(Vec::new()))
}

fn block_with_transactions(count: usize) -> Block {
    let mut block = Block::default();
    block.transactions = (0..count)
        .map(|index| {
            let mut tx = Transaction::new();
            tx.set_nonce(index as u32 + 1);
            tx.set_valid_until_block(100);
            tx.set_script(vec![0x11]);
            tx
        })
        .collect();
    block
}

// ---- engine port mocks ----------------------------------------------------

struct MockNative {
    name: &'static str,
    trigger: TriggerType,
    states: LedgerTransactionStates,
    log: Log,
}

impl NativeEnginePort for MockNative {
    fn native_on_persist(&mut self) -> Result<(), LedgerError> {
        self.log
            .lock()
            .push(format!("{}:native_on_persist", self.name));
        Ok(())
    }
    fn native_post_persist(&mut self) -> Result<(), LedgerError> {
        self.log
            .lock()
            .push(format!("{}:native_post_persist", self.name));
        Ok(())
    }
    fn record_native_persist_success(&mut self) {}
    fn set_transaction_states(&mut self, states: LedgerTransactionStates) {
        self.states = states;
    }
    fn take_transaction_states(&mut self) -> LedgerTransactionStates {
        std::mem::take(&mut self.states)
    }
    fn seed_native_contracts(&self) -> SeededNativeContracts {
        HashMap::new()
    }
    fn native_cache_handle(&self) -> NativeCacheHandle {
        Arc::new(Mutex::new(NativeContractsCache::default()))
    }
    fn execution_receipt(&mut self) -> ExecutionReceipt {
        ExecutionReceipt::native(self.trigger, VmState::HALT, 0)
    }
}

struct MockApp {
    tx_hash: UInt256,
    vm_state: VmState,
    exception: Option<String>,
    states: LedgerTransactionStates,
    log: Log,
}

impl ApplicationEnginePort for MockApp {
    fn load_script(&mut self, _script: Vec<u8>) -> Result<(), LedgerError> {
        Ok(())
    }
    fn execute_allow_fault(&mut self) -> VmState {
        self.vm_state
    }
    fn record_transaction_vm_state(&mut self, tx_hash: &UInt256, vm_state: VmState) {
        self.log
            .lock()
            .push(format!("record_vm_state:{tx_hash:?}:{vm_state:?}"));
    }
    fn set_transaction_states(&mut self, states: LedgerTransactionStates) {
        self.states = states;
    }
    fn take_transaction_states(&mut self) -> LedgerTransactionStates {
        std::mem::take(&mut self.states)
    }
    fn fee_consumed(&self) -> i64 {
        0
    }
    fn fault_exception(&self) -> Option<String> {
        self.exception.clone()
    }
    fn execution_receipt(&mut self) -> ExecutionReceipt {
        ExecutionReceipt {
            trigger: TriggerType::Application,
            transaction_hash: Some(self.tx_hash),
            vm_state: self.vm_state,
            gas_consumed: 0,
            exception: self.exception.clone(),
        }
    }
}

struct MockFactory {
    vm_states: Arc<Mutex<VecDeque<VmState>>>,
    exception: Option<String>,
    counter: Arc<AtomicUsize>,
    log: Log,
}

impl MockFactory {
    fn new(vm_states: Vec<VmState>, exception: Option<String>, log: Log) -> Self {
        Self {
            vm_states: Arc::new(Mutex::new(vm_states.into())),
            exception,
            counter: Arc::new(AtomicUsize::new(1)),
            log,
        }
    }
    fn next_hash(&self) -> UInt256 {
        let byte = (self.counter.fetch_add(1, Ordering::SeqCst) % 256) as u8;
        UInt256::from([byte; 32])
    }
}

impl EngineFactory for MockFactory {
    fn open_on_persist(
        &self,
        _snapshot: Arc<PersistenceDataCache>,
        _block: Arc<Block>,
    ) -> Result<Box<dyn NativeEnginePort>, LedgerError> {
        Ok(Box::new(MockNative {
            name: "on_persist",
            trigger: TriggerType::OnPersist,
            states: LedgerTransactionStates::default(),
            log: self.log.clone(),
        }))
    }
    fn open_application(
        &self,
        _container: Arc<dyn Verifiable>,
        _snapshot: Arc<PersistenceDataCache>,
        _block: Arc<Block>,
        _system_fee: i64,
        _seeded: &SeededNativeContracts,
        _cache: &NativeCacheHandle,
    ) -> Result<Box<dyn ApplicationEnginePort>, LedgerError> {
        let vm_state = self.vm_states.lock().pop_front().unwrap_or(VmState::HALT);
        Ok(Box::new(MockApp {
            tx_hash: self.next_hash(),
            vm_state,
            exception: self.exception.clone(),
            states: LedgerTransactionStates::default(),
            log: self.log.clone(),
        }))
    }
    fn open_post_persist(
        &self,
        _snapshot: Arc<PersistenceDataCache>,
        _block: Arc<Block>,
        _seeded: &SeededNativeContracts,
        _cache: &NativeCacheHandle,
    ) -> Result<Box<dyn NativeEnginePort>, LedgerError> {
        Ok(Box::new(MockNative {
            name: "post_persist",
            trigger: TriggerType::PostPersist,
            states: LedgerTransactionStates::default(),
            log: self.log.clone(),
        }))
    }
}

// ---- storage / runtime / evidence mocks -----------------------------------

struct MockTransaction {
    base: Arc<PersistenceDataCache>,
    commit_result: Result<(), String>,
    log: Log,
}

impl StoreTransactionPort for MockTransaction {
    fn base_snapshot(&self) -> Arc<PersistenceDataCache> {
        Arc::clone(&self.base)
    }
    fn commit(self: Box<Self>) -> Result<(), String> {
        self.log.lock().push("store_commit".to_string());
        self.commit_result.clone()
    }
}

struct MockStore {
    commit_result: Result<(), String>,
    log: Log,
}

impl SnapshotStore for MockStore {
    fn open_transaction(
        &self,
        _options: &PersistOptions,
    ) -> Result<Box<dyn StoreTransactionPort>, LedgerError> {
        Ok(Box::new(MockTransaction {
            base: Arc::new(PersistenceDataCache::new(false)),
            commit_result: self.commit_result.clone(),
            log: self.log.clone(),
        }))
    }
}

struct MockRuntime {
    record_result: Result<(), String>,
    log: Log,
}

impl RuntimeCacheSink for MockRuntime {
    fn record_block(&self, block: &Block) -> Result<(), String> {
        self.log
            .lock()
            .push(format!("record_block:{}", block.index()));
        self.record_result.clone()
    }
    fn record_tip(&self, index: u32) {
        self.log.lock().push(format!("record_tip:{index}"));
    }
    fn broadcast_block_received(&self, _hash: String, height: u32) {
        self.log.lock().push(format!("broadcast:{height}"));
    }
}

struct MockEvidence {
    capacity: usize,
    accept: bool,
    dropped: Arc<Mutex<u64>>,
    log: Log,
}

impl EvidenceSink for MockEvidence {
    fn available_capacity(&self) -> usize {
        self.capacity
    }
    fn try_emit(&self, _observation: &FaultObservation) -> bool {
        self.log.lock().push("emit".to_string());
        self.accept
    }
    fn record_dropped(&self, dropped: u64) {
        *self.dropped.lock() = dropped;
        self.log.lock().push(format!("record_dropped:{dropped}"));
    }
}

// ---- participant mock -----------------------------------------------------

struct MockParticipant {
    name: &'static str,
    opt_into_fast_sync: bool,
    precommit: Result<(), String>,
    log: Log,
}

impl CommitParticipant for MockParticipant {
    fn run_during_fast_sync(&self) -> bool {
        self.opt_into_fast_sync
    }
    fn precommit(&self, _context: &StageContext<'_>) -> Result<(), String> {
        self.log.lock().push(format!("{}:precommit", self.name));
        self.precommit.clone()
    }
    fn discard(&self, _context: &StageContext<'_>) {
        self.log.lock().push(format!("{}:discard", self.name));
    }
    fn committed(&self, _context: &CommitContext<'_>) {
        self.log.lock().push(format!("{}:committed", self.name));
    }
}

fn participant(
    name: &'static str,
    opt_into_fast_sync: bool,
    precommit: Result<(), String>,
    log: Log,
) -> Arc<dyn CommitParticipant> {
    Arc::new(MockParticipant {
        name,
        opt_into_fast_sync,
        precommit,
        log,
    })
}

// ---- tests ----------------------------------------------------------------

#[test]
fn halt_merges_and_fault_stages_bounded_evidence_after_commit() {
    let log = log();
    let dropped = Arc::new(Mutex::new(0u64));
    let store = MockStore {
        commit_result: Ok(()),
        log: log.clone(),
    };
    let factory = MockFactory::new(
        vec![VmState::HALT, VmState::FAULT],
        Some("caught".to_string()),
        log.clone(),
    );
    let runtime = MockRuntime {
        record_result: Ok(()),
        log: log.clone(),
    };
    let evidence = MockEvidence {
        capacity: 64,
        accept: true,
        dropped: dropped.clone(),
        log: log.clone(),
    };

    let receipt = persist_block(
        block_with_transactions(2),
        &store,
        &factory,
        &runtime,
        &evidence,
        &[],
        &PersistOptions::online(),
    )
    .expect("persist succeeds");

    // OnPersist + 2 transactions + PostPersist.
    assert_eq!(receipt.receipts.len(), 4);
    assert!(receipt.detailed);
    assert_eq!(receipt.staged_fault_observations, 1);
    assert_eq!(receipt.dropped_fault_observations, 0);

    let entries = log.lock().clone();
    let commit = entries
        .iter()
        .position(|e| e == "store_commit")
        .expect("commit");
    let emit = entries.iter().position(|e| e == "emit").expect("emit");
    assert!(
        emit > commit,
        "fault evidence must be exported only after the store commit"
    );
    assert!(entries.iter().any(|e| e == "broadcast:0"));
}

#[test]
fn precommit_failure_discards_staged_participants_in_reverse_order() {
    let log = log();
    let dropped = Arc::new(Mutex::new(0u64));
    let store = MockStore {
        commit_result: Ok(()),
        log: log.clone(),
    };
    let factory = MockFactory::new(vec![VmState::HALT], None, log.clone());
    let runtime = MockRuntime {
        record_result: Ok(()),
        log: log.clone(),
    };
    let evidence = MockEvidence {
        capacity: 64,
        accept: true,
        dropped: dropped.clone(),
        log: log.clone(),
    };
    let participants = vec![
        participant("a", false, Ok(()), log.clone()),
        participant("b", false, Ok(()), log.clone()),
        participant(
            "c",
            false,
            Err("injected committing failure".to_string()),
            log.clone(),
        ),
        participant("d", false, Ok(()), log.clone()),
    ];

    let err = persist_block(
        block_with_transactions(1),
        &store,
        &factory,
        &runtime,
        &evidence,
        &participants,
        &PersistOptions::online(),
    )
    .expect_err("precommit failure aborts persistence");

    // Display embeds the injected participant message verbatim.
    assert!(err.to_string().contains("injected committing failure"));
    let entries = log.lock().clone();
    // Reverse-order discard of the staging list: the failing participant c is
    // included (it may have written partial staging state), then b, then a.
    // d never reached.
    let c_discard = entries
        .iter()
        .position(|e| e == "c:discard")
        .expect("failing participant must be discarded");
    let b_discard = entries
        .iter()
        .position(|e| e == "b:discard")
        .expect("b discard");
    let a_discard = entries
        .iter()
        .position(|e| e == "a:discard")
        .expect("a discard");
    assert!(
        c_discard < b_discard && b_discard < a_discard,
        "discard must run in reverse order and start with the failing participant"
    );
    assert!(!entries.iter().any(|e| e == "d:precommit"));
    assert!(!entries.iter().any(|e| e == "store_commit"));
}

#[test]
fn store_commit_failure_carries_prefix_and_discards_in_reverse() {
    let log = log();
    let dropped = Arc::new(Mutex::new(0u64));
    let store = MockStore {
        commit_result: Err("boom".to_string()),
        log: log.clone(),
    };
    let factory = MockFactory::new(vec![VmState::HALT], None, log.clone());
    let runtime = MockRuntime {
        record_result: Ok(()),
        log: log.clone(),
    };
    let evidence = MockEvidence {
        capacity: 64,
        accept: true,
        dropped: dropped.clone(),
        log: log.clone(),
    };
    let participants = vec![
        participant("a", false, Ok(()), log.clone()),
        participant("b", false, Ok(()), log.clone()),
    ];

    let err = persist_block(
        block_with_transactions(1),
        &store,
        &factory,
        &runtime,
        &evidence,
        &participants,
        &PersistOptions::online(),
    )
    .expect_err("store commit failure aborts persistence");

    // Display carries the canonical store commit prefix.
    assert_eq!(
        err.to_string(),
        "failed to commit store cache for block 0: boom"
    );
    let entries = log.lock().clone();
    let b_discard = entries
        .iter()
        .position(|e| e == "b:discard")
        .expect("b discard");
    let a_discard = entries
        .iter()
        .position(|e| e == "a:discard")
        .expect("a discard");
    assert!(b_discard < a_discard);
    assert!(!entries.iter().any(|e| e == "record_block:0"));
}

#[test]
fn unexpected_vm_state_aborts_before_commit() {
    let log = log();
    let dropped = Arc::new(Mutex::new(0u64));
    let store = MockStore {
        commit_result: Ok(()),
        log: log.clone(),
    };
    let factory = MockFactory::new(vec![VmState::BREAK], None, log.clone());
    let runtime = MockRuntime {
        record_result: Ok(()),
        log: log.clone(),
    };
    let evidence = MockEvidence {
        capacity: 64,
        accept: true,
        dropped: dropped.clone(),
        log: log.clone(),
    };

    let err = persist_block(
        block_with_transactions(1),
        &store,
        &factory,
        &runtime,
        &evidence,
        &[],
        &PersistOptions::online(),
    )
    .expect_err("non HALT/FAULT VM state aborts");

    assert!(matches!(err, LedgerError::UnexpectedVmState { .. }));
    assert!(err.to_string().contains("unexpected transaction VM state"));
    assert!(!log.lock().iter().any(|e| e == "store_commit"));
}

#[test]
fn post_commit_evidence_export_is_bounded_and_never_fails() {
    let log = log();
    let dropped = Arc::new(Mutex::new(0u64));
    let store = MockStore {
        commit_result: Ok(()),
        log: log.clone(),
    };
    // Eight FAULT transactions but only four staging slots and a rejecting sink.
    let factory = MockFactory::new(vec![VmState::FAULT; 8], None, log.clone());
    let runtime = MockRuntime {
        record_result: Ok(()),
        log: log.clone(),
    };
    let evidence = MockEvidence {
        capacity: 4,
        accept: false,
        dropped: dropped.clone(),
        log: log.clone(),
    };

    let receipt = persist_block(
        block_with_transactions(8),
        &store,
        &factory,
        &runtime,
        &evidence,
        &[],
        &PersistOptions::online(),
    )
    .expect("a rejecting sink cannot fail persistence");

    // Bounded staging: min(capacity=4, MAX=64) = 4 staged; 4 overflow-dropped,
    // plus 4 rejected by the sink on export = 8 total dropped.
    assert_eq!(receipt.staged_fault_observations, 4);
    assert_eq!(receipt.dropped_fault_observations, 8);
    assert_eq!(*dropped.lock(), 8);
    // Commit still succeeded and the tip/cache advanced.
    assert!(log.lock().iter().any(|e| e == "store_commit"));
    assert!(log.lock().iter().any(|e| e == "record_block:0"));
}

#[test]
fn fast_sync_without_opt_in_suppresses_receipts_but_records_vm_state() {
    let log = log();
    let dropped = Arc::new(Mutex::new(0u64));
    let store = MockStore {
        commit_result: Ok(()),
        log: log.clone(),
    };
    let factory = MockFactory::new(vec![VmState::HALT; 2], None, log.clone());
    let runtime = MockRuntime {
        record_result: Ok(()),
        log: log.clone(),
    };
    let evidence = MockEvidence {
        capacity: 64,
        accept: true,
        dropped: dropped.clone(),
        log: log.clone(),
    };
    // A participant that does NOT opt into fast-sync: detailed emission stays off,
    // but committed() must still run for it (step 15a).
    let participants = vec![participant("a", false, Ok(()), log.clone())];

    let receipt = persist_block(
        block_with_transactions(2),
        &store,
        &factory,
        &runtime,
        &evidence,
        &participants,
        &PersistOptions::fast_sync(),
    )
    .expect("fast-sync persist succeeds");

    assert!(!receipt.detailed);
    assert!(receipt.receipts.is_empty());
    let entries = log.lock().clone();
    // VM state recorded unconditionally for both transactions despite no receipts.
    assert_eq!(
        entries
            .iter()
            .filter(|e| e.starts_with("record_vm_state:"))
            .count(),
        2
    );
    // precommit skipped (fast-sync, no opt-in) but committed still runs.
    assert!(!entries.iter().any(|e| e == "a:precommit"));
    assert!(entries.iter().any(|e| e == "a:committed"));
    // No plugin broadcast while detailed emission is suppressed.
    assert!(!entries.iter().any(|e| e.starts_with("broadcast:")));
}

#[test]
fn fast_sync_with_opt_in_participant_emits_detailed_receipts() {
    let log = log();
    let dropped = Arc::new(Mutex::new(0u64));
    let store = MockStore {
        commit_result: Ok(()),
        log: log.clone(),
    };
    let factory = MockFactory::new(vec![VmState::HALT], None, log.clone());
    let runtime = MockRuntime {
        record_result: Ok(()),
        log: log.clone(),
    };
    let evidence = MockEvidence {
        capacity: 64,
        accept: true,
        dropped: dropped.clone(),
        log: log.clone(),
    };
    let participants = vec![participant("a", true, Ok(()), log.clone())];

    let receipt = persist_block(
        block_with_transactions(1),
        &store,
        &factory,
        &runtime,
        &evidence,
        &participants,
        &PersistOptions::fast_sync(),
    )
    .expect("fast-sync persist succeeds");

    // An opt-in participant forces detailed emission even under fast-sync.
    assert!(receipt.detailed);
    assert_eq!(receipt.receipts.len(), 3);
    let entries = log.lock().clone();
    assert!(entries.iter().any(|e| e == "a:precommit"));
    assert!(entries.iter().any(|e| e == "a:committed"));
    assert!(entries.iter().any(|e| e == "broadcast:0"));
}

#[test]
fn offline_import_advances_tip_without_runtime_block_cache() {
    let log = log();
    let dropped = Arc::new(Mutex::new(0u64));
    let store = MockStore {
        commit_result: Ok(()),
        log: log.clone(),
    };
    let factory = MockFactory::new(vec![VmState::HALT], None, log.clone());
    let runtime = MockRuntime {
        record_result: Ok(()),
        log: log.clone(),
    };
    let evidence = MockEvidence {
        capacity: 64,
        accept: true,
        dropped: dropped.clone(),
        log: log.clone(),
    };

    let receipt = persist_block(
        block_with_transactions(1),
        &store,
        &factory,
        &runtime,
        &evidence,
        &[],
        &PersistOptions::offline_import(),
    )
    .expect("offline import succeeds");

    assert!(receipt.detailed);
    let entries = log.lock().clone();
    assert!(entries.iter().any(|e| e == "record_tip:0"));
    assert!(!entries.iter().any(|e| e.starts_with("record_block:")));
}

#[test]
fn runtime_cache_failure_after_durable_commit_reports_stale_cache_without_rollback() {
    let log = log();
    let dropped = Arc::new(Mutex::new(0u64));
    let store = MockStore {
        commit_result: Ok(()),
        log: log.clone(),
    };
    let factory = MockFactory::new(vec![VmState::HALT], None, log.clone());
    let runtime = MockRuntime {
        record_result: Err("cache offline".to_string()),
        log: log.clone(),
    };
    let evidence = MockEvidence {
        capacity: 64,
        accept: true,
        dropped: dropped.clone(),
        log: log.clone(),
    };
    let participants = vec![participant("a", false, Ok(()), log.clone())];

    let err = persist_block(
        block_with_transactions(1),
        &store,
        &factory,
        &runtime,
        &evidence,
        &participants,
        &PersistOptions::online(),
    )
    .expect_err("runtime cache failure is still reported");

    // The error must be the distinct *post-commit* variant: durable state
    // advanced, only the local runtime view is stale.
    assert!(matches!(err, LedgerError::PostCommitCacheFailure { .. }));
    assert_eq!(
        err.to_string(),
        "block 0 was durably committed but the runtime cache update failed: cache offline"
    );
    let entries = log.lock().clone();
    assert!(entries.iter().any(|e| e == "store_commit"));
    // No rollback of already-committed data: the participant is finalized, not
    // discarded, and post-commit side effects still run.
    assert!(!entries.iter().any(|e| e == "a:discard"));
    assert!(entries.iter().any(|e| e == "a:committed"));
    assert!(entries.iter().any(|e| e == "broadcast:0"));
}
