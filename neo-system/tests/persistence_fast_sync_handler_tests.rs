//! Integration tests for the canonical `NeoSystem` persistence pipeline and
//! its fast-sync handler / fault-observation behavior.

use neo_native_contracts::ledger_contract::{HashOrIndex, LedgerContract};
use neo_primitives::{UInt160, UInt256, WitnessScope};
use neo_protocol::execution::ApplicationExecuted;
use neo_protocol::execution::Block as LedgerBlock;
use neo_protocol::payloads::signer::Signer;
use neo_protocol::protocol_settings::ProtocolSettings;
use neo_protocol::witness::Witness;
use neo_smart_contract::error::CoreError;
use neo_storage::persistence::DataCache;
use neo_system::events::handlers::CoreResult;
use neo_system::i_event_handlers::{CommittedHandler, CommittingHandler};
use neo_system::neo_system::{
    NeoSystem, TRANSACTION_EXECUTION_OBSERVATION_RULE_VERSION,
    TRANSACTION_EXECUTION_OBSERVATION_SCHEMA_VERSION, TransactionExecutionExceptionCategory,
    TransactionExecutionObservation, TransactionExecutionStage,
};
use neo_system::network::p2p::payloads::block::Block as PayloadBlock;
use neo_system::network::p2p::payloads::header::Header;
use neo_system::network::p2p::payloads::transaction::Transaction;
use neo_vm::{OpCode, VmState};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

struct FastSyncCaptureHandler {
    observed_len: Arc<AtomicUsize>,
}

struct FailingCommittingHandler;

struct FillEvidenceQueueHandler {
    sink: mpsc::Sender<TransactionExecutionObservation>,
    observation: TransactionExecutionObservation,
    count: usize,
}

impl CommittingHandler for FastSyncCaptureHandler {
    fn run_during_fast_sync(&self) -> bool {
        true
    }

    fn blockchain_committing_handler(
        &self,
        _system: &dyn std::any::Any,
        _block: &LedgerBlock,
        _snapshot: &DataCache,
        application_executed_list: &[ApplicationExecuted],
    ) {
        self.observed_len
            .store(application_executed_list.len(), Ordering::Relaxed);
    }
}

impl CommittingHandler for FailingCommittingHandler {
    fn blockchain_committing_handler(
        &self,
        _system: &dyn std::any::Any,
        _block: &LedgerBlock,
        _snapshot: &DataCache,
        _application_executed_list: &[ApplicationExecuted],
    ) {
    }

    fn try_blockchain_committing_handler(
        &self,
        _system: &dyn std::any::Any,
        _block: &LedgerBlock,
        _snapshot: &DataCache,
        _application_executed_list: &[ApplicationExecuted],
    ) -> CoreResult<()> {
        Err(CoreError::system("injected committing failure").into())
    }
}

impl CommittingHandler for FillEvidenceQueueHandler {
    fn blockchain_committing_handler(
        &self,
        _system: &dyn std::any::Any,
        _block: &LedgerBlock,
        _snapshot: &DataCache,
        _application_executed_list: &[ApplicationExecuted],
    ) {
        for _ in 0..self.count {
            self.sink
                .try_send(self.observation.clone())
                .expect("fill evidence queue during commit");
        }
    }
}

#[derive(Default)]
struct LifecycleCapture {
    events: Mutex<Vec<&'static str>>,
}

impl CommittingHandler for LifecycleCapture {
    fn blockchain_committing_handler(
        &self,
        _system: &dyn std::any::Any,
        _block: &LedgerBlock,
        _snapshot: &DataCache,
        _application_executed_list: &[ApplicationExecuted],
    ) {
        self.events
            .lock()
            .expect("lifecycle events")
            .push("committing");
    }
}

impl CommittedHandler for LifecycleCapture {
    fn blockchain_committed_handler(&self, _system: &dyn std::any::Any, _block: &LedgerBlock) {
        self.events
            .lock()
            .expect("lifecycle events")
            .push("committed");
    }
}

fn faulting_block(system: &NeoSystem, transaction_count: usize) -> PayloadBlock {
    let transactions = (0..transaction_count)
        .map(|index| {
            let mut transaction = Transaction::new();
            transaction.set_nonce(index as u32 + 1);
            transaction.set_valid_until_block(100);
            transaction.set_script(vec![OpCode::ABORT.byte()]);
            transaction.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::GLOBAL)]);
            transaction.set_witnesses(vec![Witness::new()]);
            transaction
        })
        .collect();

    let genesis = system.genesis_block().as_ref().clone();
    let mut header = Header::new();
    header.set_index(1);
    header.set_prev_hash(genesis.hash());
    header.set_next_consensus(UInt160::zero());
    header.set_timestamp(1);
    header.witness = Witness::new();

    let mut block = PayloadBlock::new();
    block.header = header;
    block.transactions = transactions;
    block.try_rebuild_merkle_root().expect("merkle root");
    block
}

fn observation(transaction_hash: UInt256) -> TransactionExecutionObservation {
    TransactionExecutionObservation {
        transaction_hash,
        block_height: 0,
        block_hash: UInt256::zero(),
        execution_stage: TransactionExecutionStage::Application,
        vm_state: VmState::FAULT,
        exception_category: TransactionExecutionExceptionCategory::CaughtVmException,
        gas_consumed: 0,
        schema_version: TRANSACTION_EXECUTION_OBSERVATION_SCHEMA_VERSION,
        rule_version: TRANSACTION_EXECUTION_OBSERVATION_RULE_VERSION,
        observed_at_unix_ms: 0,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn fast_sync_opt_in_handler_receives_application_executed_data() {
    let system = NeoSystem::new(ProtocolSettings::mainnet(), None, None).expect("system");
    system.context().enable_fast_sync_mode();

    let observed_len = Arc::new(AtomicUsize::new(usize::MAX));
    system
        .register_committing_handler(Arc::new(FastSyncCaptureHandler {
            observed_len: Arc::clone(&observed_len),
        }))
        .expect("register handler");

    let mut block = PayloadBlock::new();
    let mut header = Header::new();
    header.set_index(1);
    header.set_prev_hash(UInt256::zero());
    header.set_merkle_root(UInt256::zero());
    header.set_next_consensus(UInt160::zero());
    header.set_timestamp(1);
    header.witness = Witness::new();
    block.header = header;
    block.transactions = Vec::new();

    system
        .persist_block(LedgerBlock::from(&block))
        .expect("persist block");

    assert!(
        observed_len.load(Ordering::Relaxed) > 0,
        "fast-sync handlers that opt in should still receive application execution data"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn persisted_block_and_transaction_state_are_readable_from_ledger_contract() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
    let block = faulting_block(&system, 1);
    let block_hash = LedgerBlock::from(&block).hash();
    let transaction_hash = block.transactions[0].hash();

    system
        .persist_block(LedgerBlock::from(&block))
        .expect("persist block");

    let store_cache = system.store_cache();
    let ledger_contract = LedgerContract::new();
    assert_eq!(ledger_contract.current_index(&store_cache).unwrap(), 1);
    assert_eq!(
        ledger_contract.current_hash(&store_cache).unwrap(),
        block_hash
    );
    let persisted = ledger_contract
        .get_block(&store_cache, HashOrIndex::Index(1))
        .expect("read persisted block")
        .expect("block must be present");
    assert_eq!(persisted.hash(), block_hash);
    assert_eq!(persisted.transactions.len(), 1);
    assert_eq!(persisted.transactions[0].hash(), transaction_hash);

    let transaction_state = ledger_contract
        .get_transaction_state(&store_cache, &transaction_hash)
        .expect("read transaction state")
        .expect("transaction state must be present");
    assert_eq!(transaction_state.block_index(), 1);
    assert_eq!(transaction_state.vm_state(), VmState::FAULT);
    assert_eq!(system.current_block_index(), 1);
    system.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "multi_thread")]
async fn genesis_is_persisted_and_readable_as_a_complete_roundtrip() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
    let genesis_hash = system.genesis_block().hash();
    let store_cache = system.store_cache();
    let ledger_contract = LedgerContract::new();

    assert_eq!(ledger_contract.current_index(&store_cache).unwrap(), 0);
    assert_eq!(
        ledger_contract.current_hash(&store_cache).unwrap(),
        genesis_hash
    );
    let persisted = ledger_contract
        .get_block(&store_cache, HashOrIndex::Index(0))
        .expect("read genesis")
        .expect("genesis must be present");
    assert_eq!(persisted.hash(), genesis_hash);
    assert_eq!(system.current_block_index(), 0);
    system.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "multi_thread")]
async fn committing_runs_once_before_committed() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
    let capture = Arc::new(LifecycleCapture::default());
    system
        .register_committing_handler(capture.clone())
        .expect("register committing handler");
    system
        .register_committed_handler(capture.clone())
        .expect("register committed handler");

    system
        .persist_block(LedgerBlock::from(&faulting_block(&system, 0)))
        .expect("persist block");

    assert_eq!(
        capture.events.lock().expect("lifecycle events").as_slice(),
        &["committing", "committed"]
    );
    system.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "multi_thread")]
async fn persist_block_stops_before_commit_when_committing_handler_fails() {
    let system = NeoSystem::new(ProtocolSettings::mainnet(), None, None).expect("system");
    let genesis_hash = system.genesis_block().hash();
    let block = faulting_block(&system, 1);
    let transaction_hash = block.transactions[0].hash();
    let (sink, mut receiver) = mpsc::channel(1);
    system
        .register_transaction_execution_sink(sink)
        .expect("register sink");
    system
        .register_committing_handler(Arc::new(FailingCommittingHandler))
        .expect("register handler");

    let Err(err) = system.persist_block(LedgerBlock::from(&block)) else {
        panic!("committing handler should block persistence")
    };

    assert!(err.to_string().contains("injected committing failure"));
    assert_eq!(system.ledger_context().current_height(), 0);
    let store_cache = system.store_cache();
    let ledger_contract = LedgerContract::new();
    assert_eq!(ledger_contract.current_index(&store_cache).unwrap(), 0);
    assert_eq!(
        ledger_contract.current_hash(&store_cache).unwrap(),
        genesis_hash
    );
    assert!(
        ledger_contract
            .get_block(&store_cache, HashOrIndex::Index(1))
            .expect("read rejected block")
            .is_none()
    );
    assert!(
        ledger_contract
            .get_transaction_state(&store_cache, &transaction_hash)
            .expect("read rejected transaction")
            .is_none()
    );
    assert!(receiver.try_recv().is_err());
    assert_eq!(
        system
            .context()
            .dropped_transaction_execution_observation_count(),
        0
    );
    system.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "multi_thread")]
async fn successful_commit_emits_fault_observation() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
    let (sink, mut receiver) = mpsc::channel(1);
    system
        .register_transaction_execution_sink(sink)
        .expect("register sink");

    system
        .persist_block(LedgerBlock::from(&faulting_block(&system, 1)))
        .expect("persist faulting block");

    let observed = receiver.try_recv().expect("fault observation");
    assert_eq!(observed.block_height, 1);
    assert_eq!(observed.vm_state, VmState::FAULT);
    assert_eq!(
        observed.execution_stage,
        TransactionExecutionStage::Application
    );
    assert_eq!(
        observed.schema_version,
        TRANSACTION_EXECUTION_OBSERVATION_SCHEMA_VERSION
    );
    assert_eq!(
        observed.rule_version,
        TRANSACTION_EXECUTION_OBSERVATION_RULE_VERSION
    );
    assert!(receiver.try_recv().is_err());
    assert_eq!(
        system
            .context()
            .dropped_transaction_execution_observation_count(),
        0
    );
    system.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "multi_thread")]
async fn fault_observation_staging_is_strictly_bounded_after_successful_commit() {
    const STAGING_LIMIT: usize = 64;
    const TRANSACTION_COUNT: usize = STAGING_LIMIT * 4;

    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
    let (sink, mut receiver) = mpsc::channel(STAGING_LIMIT + 8);
    system
        .register_transaction_execution_sink(sink)
        .expect("register sink");

    system
        .persist_block(LedgerBlock::from(&faulting_block(
            &system,
            TRANSACTION_COUNT,
        )))
        .expect("persist faulting block");

    let mut observations = Vec::new();
    while let Ok(observation) = receiver.try_recv() {
        observations.push(observation);
    }
    assert_eq!(observations.len(), STAGING_LIMIT);
    assert!(observations.iter().all(|observation| {
        observation.block_height == 1
            && observation.vm_state == VmState::FAULT
            && observation.execution_stage == TransactionExecutionStage::Application
    }));
    assert_eq!(
        system
            .context()
            .dropped_transaction_execution_observation_count(),
        (TRANSACTION_COUNT - STAGING_LIMIT) as u64
    );
    assert_eq!(system.current_block_index(), 1);
    system.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "multi_thread")]
async fn unavailable_sinks_do_not_change_canonical_fault_results() {
    let unregistered_system =
        NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
    unregistered_system
        .persist_block(LedgerBlock::from(&faulting_block(&unregistered_system, 8)))
        .expect("missing sink cannot fail persistence");
    assert_eq!(unregistered_system.current_block_index(), 1);
    assert_eq!(
        unregistered_system
            .context()
            .dropped_transaction_execution_observation_count(),
        8
    );
    unregistered_system.shutdown().await.expect("shutdown");

    let full_system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
    let (full_sink, mut full_receiver) = mpsc::channel(1);
    let sentinel = observation(UInt256::from([0xee; 32]));
    full_sink
        .try_send(sentinel.clone())
        .expect("fill evidence queue");
    full_system
        .register_transaction_execution_sink(full_sink)
        .expect("register full sink");

    full_system
        .persist_block(LedgerBlock::from(&faulting_block(&full_system, 8)))
        .expect("full sink cannot fail persistence");
    assert_eq!(full_system.current_block_index(), 1);
    assert_eq!(full_receiver.try_recv().expect("sentinel"), sentinel);
    assert!(full_receiver.try_recv().is_err());
    assert_eq!(
        full_system
            .context()
            .dropped_transaction_execution_observation_count(),
        8
    );
    full_system.shutdown().await.expect("shutdown");

    let closed_system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
    let (closed_sink, closed_receiver) = mpsc::channel(1);
    drop(closed_receiver);
    closed_system
        .register_transaction_execution_sink(closed_sink)
        .expect("register closed sink");

    closed_system
        .persist_block(LedgerBlock::from(&faulting_block(&closed_system, 8)))
        .expect("closed sink cannot fail persistence");
    assert_eq!(closed_system.current_block_index(), 1);
    assert_eq!(
        closed_system
            .context()
            .dropped_transaction_execution_observation_count(),
        8
    );
    closed_system.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "multi_thread")]
async fn capacity_shrink_after_staging_drops_without_affecting_commit() {
    const CAPACITY: usize = 4;
    const TRANSACTION_COUNT: usize = CAPACITY * 2;

    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
    let (sink, mut receiver) = mpsc::channel(CAPACITY);
    system
        .register_transaction_execution_sink(sink.clone())
        .expect("register sink");
    system
        .register_committing_handler(Arc::new(FillEvidenceQueueHandler {
            sink,
            observation: observation(UInt256::from([0xdd; 32])),
            count: CAPACITY,
        }))
        .expect("register queue filler");

    system
        .persist_block(LedgerBlock::from(&faulting_block(
            &system,
            TRANSACTION_COUNT,
        )))
        .expect("queue capacity changes cannot fail persistence");

    assert_eq!(system.current_block_index(), 1);
    for _ in 0..CAPACITY {
        assert_eq!(
            receiver.try_recv().expect("sentinel"),
            observation(UInt256::from([0xdd; 32]))
        );
    }
    assert!(receiver.try_recv().is_err());
    assert_eq!(
        system
            .context()
            .dropped_transaction_execution_observation_count(),
        TRANSACTION_COUNT as u64
    );
    system.shutdown().await.expect("shutdown");
}
