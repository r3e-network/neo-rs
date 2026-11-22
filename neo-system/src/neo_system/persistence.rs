//! Concrete adapters between `NeoSystem` and the canonical ledger persistence pipeline.

use std::cell::Cell;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

thread_local! {
    /// Diagnostic-only block classification shared with nested MPT/store sinks.
    /// The guard is installed only when persistence profiling is explicitly enabled.
    static PROFILE_EMPTY_BLOCK: Cell<bool> = const { Cell::new(false) };
}

use super::context::{
    NeoSystemContext, TRANSACTION_EXECUTION_OBSERVATION_RULE_VERSION,
    TRANSACTION_EXECUTION_OBSERVATION_SCHEMA_VERSION, TransactionExecutionExceptionCategory,
    TransactionExecutionObservation, TransactionExecutionStage,
};
use super::core::NeoSystem;
use neo_actors::EventStreamHandle;
use neo_crypto::mpt_trie::MptProfileSnapshot;
use neo_ledger::persistence::{
    ApplicationEnginePort, CommitParticipant, EngineFactory, EvidenceSink, ExecutionReceipt,
    FaultCategory, FaultObservation, LedgerError, NativeCacheHandle, NativeEnginePort,
    PersistOptions, PersistenceProfiler, PersistenceStage, RuntimeCacheSink, SeededNativeContracts,
    SnapshotStore, SpeculativeExecution, StoreTransactionPort, TransactionLoopStrategy,
};
use neo_primitives::{CallFlags, TriggerType, UInt160, UInt256, Verifiable};
use neo_protocol::execution::{ApplicationExecuted, Block};
use neo_smart_contract::application_engine::execution_strategy::{
    CanonicalVerificationSource, ExecutionMode, ExecutionReport, ExecutionRequest,
    ExecutionStrategy, ExecutionStrategyObserver, TransferParams, execute_application_script,
    parse_fast_path_candidate,
};
use neo_smart_contract::application_engine::{
    ApplicationEngine, EngineNativeServices, LedgerTransactionStates, TEST_MODE_GAS,
};
use neo_smart_contract::error::{CoreError, CoreResult};
use neo_storage::persistence::data_cache::DataCacheConfig;
use neo_storage::persistence::{
    PersistenceDataCache, StoreCommitProfiler, StoreCommitStage, StoreTransaction, store::Store,
};
use neo_vm::VmState;
use parking_lot::Mutex;

mod block_flow;
mod profile;
mod snapshot_store;
#[cfg(test)]
mod tests;
mod transaction;
mod write_back;

pub(crate) use block_flow::install_state_root_profile_sink;
pub use profile::{
    EmptyBlockProfileSnapshot, ExecutionProfileSnapshot, ExecutionStats, FastPathStatsSnapshot,
    ParallelExecutionSnapshot,
};

const PROFILE_SNAPSHOT_OPEN: usize = 0;
const PROFILE_TRANSACTION_HASH: usize = 1;
const PROFILE_ON_PERSIST: usize = 2;
const PROFILE_TRANSACTION_LOOP: usize = 3;
const PROFILE_TRANSACTION_VM: usize = 4;
const PROFILE_POST_PERSIST: usize = 5;
const PROFILE_PARTICIPANT_PRECOMMIT: usize = 6;
const PROFILE_STORE_COMMIT: usize = 7;
const PROFILE_RUNTIME_UPDATE: usize = 8;
const PROFILE_POST_COMMIT: usize = 9;
const PROFILE_STATE_CHANGE_EXTRACTION: usize = 10;
const PROFILE_MPT: usize = 11;
const PROFILE_STORE_WRITER_SNAPSHOT_OPEN: usize = 12;
const PROFILE_STORE_CHANGE_APPLICATION: usize = 13;
const PROFILE_STORE_WRITE_BATCH_BUILD: usize = 14;
const PROFILE_STORE_DATABASE_WRITE: usize = 15;
const PROFILE_STORE_CACHE_INVALIDATION: usize = 16;
const PROFILE_MPT_UPDATE: usize = 17;
const PROFILE_MPT_COMMIT: usize = 18;
const PROFILE_MPT_ROOT_HASH: usize = 19;
const PROFILE_MPT_HASH: usize = 20;
const PROFILE_MPT_SERIALIZATION: usize = 21;
const PROFILE_MPT_HASH256: usize = 22;
const PROFILE_MPT_NODE_CACHE: usize = 23;
const PROFILE_MPT_NODE_READ: usize = 24;
const PROFILE_MPT_NODE_DESERIALIZE: usize = 25;
const PROFILE_MPT_NODE_ENCODE: usize = 26;
const PROFILE_MPT_NODE_WRITE: usize = 27;
const PROFILE_STAGE_COUNT: usize = 28;

/// How the profiled persistence stages nest inside one another.
///
/// Every stage in [`ExecutionProfileSnapshot`] is measured where the pipeline
/// already does the work; several of them therefore overlap. Consumers that
/// publish these timings (for example the offline `.acc` import summary) must
/// label the nesting instead of adding stages together, because a sum would
/// count the same wall-clock time more than once:
///
/// - `transaction_vm` runs inside `transaction_loop`;
/// - `state_change_extraction` and `mpt` run inside `participant_precommit`
///   (the state-root staging path);
/// - `store_database_write` runs inside `store_commit` (the chain commit).
///
/// All requested visibilities already exist as stages; no additional timers are
/// needed to observe snapshot creation, OnPersist, the transaction loop,
/// PostPersist, state-root staging, or the chain commit.
pub const PERSISTENCE_STAGE_NESTING: &str = "transaction_vm<transaction_loop; state_change_extraction+mpt<participant_precommit; \
     store_database_write<store_commit";

use crate::adapters::HandlerParticipantBridge;
use crate::events::PluginEvent;

fn engine_error(error: impl std::fmt::Display) -> LedgerError {
    LedgerError::Engine {
        message: error.to_string(),
    }
}

struct SystemRuntimeCache {
    context: Arc<NeoSystemContext>,
}

impl RuntimeCacheSink for SystemRuntimeCache {
    fn record_block(&self, block: &Block) -> Result<(), String> {
        self.context
            .record_block(block.to_payload_block())
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn record_tip(&self, index: u32) {
        self.context.ledger_handle().record_tip(index);
    }

    fn broadcast_block_received(&self, block_hash: String, block_height: u32) {
        self.context
            .broadcast_plugin_event(PluginEvent::BlockReceived {
                block_hash,
                block_height,
            });
    }
}

struct SystemEvidenceSink {
    context: Arc<NeoSystemContext>,
}

impl EvidenceSink for SystemEvidenceSink {
    fn available_capacity(&self) -> usize {
        self.context.transaction_execution_sink_available_capacity()
    }

    fn try_emit(&self, observation: &FaultObservation) -> bool {
        let observed_at_unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX);
        let exception_category = match observation.category {
            FaultCategory::CaughtVmException => {
                TransactionExecutionExceptionCategory::CaughtVmException
            }
            FaultCategory::VmFault => TransactionExecutionExceptionCategory::VmFault,
        };
        self.context
            .try_emit_transaction_execution_observation(TransactionExecutionObservation {
                transaction_hash: observation.transaction_hash,
                block_height: observation.block_height,
                block_hash: observation.block_hash,
                execution_stage: TransactionExecutionStage::Application,
                vm_state: observation.vm_state,
                exception_category,
                gas_consumed: observation.gas_consumed,
                schema_version: TRANSACTION_EXECUTION_OBSERVATION_SCHEMA_VERSION,
                rule_version: TRANSACTION_EXECUTION_OBSERVATION_RULE_VERSION,
                observed_at_unix_ms,
            })
    }

    fn record_dropped(&self, dropped: u64) {
        self.context
            .record_dropped_transaction_execution_observations(
                usize::try_from(dropped).unwrap_or(usize::MAX),
            );
    }
}

#[derive(Default)]
struct ExecutionRecorder {
    executions: Mutex<Vec<ApplicationExecuted>>,
    event_stream: Mutex<Option<EventStreamHandle>>,
}

impl ExecutionRecorder {
    fn with_event_stream(event_stream: EventStreamHandle) -> Self {
        Self {
            executions: Mutex::new(Vec::new()),
            event_stream: Mutex::new(Some(event_stream)),
        }
    }

    /// Projects the receipt the canonical serial loop would report for `engine`.
    ///
    /// This performs no bookkeeping: it neither marks the transaction state nor
    /// publishes an observation, so a speculative run can ask for the receipt
    /// before its commit order is known.
    fn receipt(engine: &mut ApplicationEngine) -> ExecutionReceipt {
        let transaction_hash = engine
            .script_container()
            .and_then(|container| {
                container
                    .as_any()
                    .downcast_ref::<neo_protocol::payloads::Transaction>()
                    .cloned()
            })
            .as_ref()
            .map(neo_protocol::payloads::Transaction::hash);
        ExecutionReceipt {
            trigger: engine.trigger(),
            transaction_hash,
            vm_state: engine.state(),
            gas_consumed: engine.fee_consumed(),
            exception: engine.fault_exception().map(str::to_owned),
        }
    }

    /// Builds the observation record for `engine` and marks the transaction state.
    ///
    /// The returned record is *not* published; callers either [`Self::publish`] it
    /// immediately (serial path) or hold it until the ordered validation releases it
    /// (speculative path), which is what keeps observation order equal to block order.
    fn project(&self, engine: &mut ApplicationEngine) -> (ApplicationExecuted, ExecutionReceipt) {
        let receipt = Self::receipt(engine);
        if let Some(hash) = receipt.transaction_hash {
            let state = engine.state();
            engine.record_transaction_vm_state(&hash, state);
        }
        let transaction = engine.script_container().and_then(|container| {
            container
                .as_any()
                .downcast_ref::<neo_protocol::payloads::Transaction>()
                .cloned()
        });
        let executed = ApplicationExecuted::new(
            transaction,
            receipt.trigger,
            receipt.vm_state,
            receipt.exception.clone(),
            receipt.gas_consumed,
            engine.result_stack().to_vec(),
            engine.notifications().to_vec(),
            engine.logs().to_vec(),
        );
        (executed, receipt)
    }

    /// Releases one observation record to the event stream and the block snapshot.
    fn publish(&self, executed: ApplicationExecuted) {
        if let Some(event_stream) = self.event_stream.lock().as_ref() {
            event_stream.publish(executed.clone());
        }
        self.executions.lock().push(executed);
    }

    fn capture(&self, engine: &mut ApplicationEngine) -> ExecutionReceipt {
        let (executed, receipt) = self.project(engine);
        self.publish(executed);
        receipt
    }

    fn snapshot(&self) -> Vec<ApplicationExecuted> {
        self.executions.lock().clone()
    }
}

struct SystemEngineFactory {
    settings: neo_protocol::ProtocolSettings,
    native_services: EngineNativeServices,
    native_method_cache: NativeCacheHandle,
    recorder: Arc<ExecutionRecorder>,
    execution_mode: ExecutionMode,
    stats: Arc<ExecutionStats>,
    // A factory is constructed for exactly one persist_block_internal call.
    // All engines for that block share one immutable payload projection.
    payload_block: std::sync::OnceLock<Arc<neo_protocol::payloads::Block>>,
}

impl SystemEngineFactory {
    fn payload_block(&self, block: &Block) -> Arc<neo_protocol::payloads::Block> {
        Arc::clone(
            self.payload_block
                .get_or_init(|| Arc::new(block.to_payload_block())),
        )
    }

    fn engine_port(&self, mut engine: ApplicationEngine) -> SystemEnginePort {
        engine.set_profiler(Some(Arc::clone(&self.stats)
            as Arc<
                dyn neo_smart_contract::application_engine::ApplicationEngineProfiler,
            >));
        SystemEnginePort {
            engine,
            recorder: Arc::clone(&self.recorder),
            execution_mode: self.execution_mode,
            stats: Arc::clone(&self.stats),
            script: None,
            transfer: None,
            verification: None,
            verification_error: None,
            pending_capture: None,
        }
    }
}

impl EngineFactory for SystemEngineFactory {
    fn open_on_persist(
        &self,
        snapshot: Arc<PersistenceDataCache>,
        block: Arc<Block>,
    ) -> Result<Box<dyn NativeEnginePort>, LedgerError> {
        let engine = ApplicationEngine::new_with_shared_block(
            TriggerType::OnPersist,
            None,
            snapshot,
            Some(self.payload_block(&block)),
            self.settings.clone(),
            TEST_MODE_GAS,
            None,
            self.native_services.clone(),
        )
        .map_err(engine_error)?
        .with_native_contract_cache_handle(Arc::clone(&self.native_method_cache));
        Ok(Box::new(self.engine_port(engine)))
    }

    fn open_application(
        &self,
        container: Arc<dyn Verifiable>,
        snapshot: Arc<PersistenceDataCache>,
        block: Arc<Block>,
        system_fee: i64,
        seeded: &SeededNativeContracts,
        cache: &NativeCacheHandle,
    ) -> Result<Box<dyn ApplicationEnginePort>, LedgerError> {
        let payload_block = self.payload_block(&block);
        // Only `verify` dual-executes, so only it pays for an isolated source.
        let verification =
            self.execution_mode
                .requires_isolated_source()
                .then(|| VerificationContext {
                    container: Arc::clone(&container),
                    snapshot: Arc::clone(&snapshot),
                    payload_block: Arc::clone(&payload_block),
                    system_fee,
                    seeded: seeded.clone(),
                    cache: Arc::clone(cache),
                    settings: self.settings.clone(),
                    native_services: self.native_services.clone(),
                });
        let engine = ApplicationEngine::new_with_preloaded_native_shared(
            TriggerType::Application,
            Some(container),
            snapshot,
            Some(payload_block),
            self.settings.clone(),
            system_fee,
            seeded.clone(),
            Arc::clone(cache),
            None,
            self.native_services.clone(),
        )
        .map_err(engine_error)?;
        let mut port = self.engine_port(engine);
        port.verification = verification;
        Ok(Box::new(port))
    }

    fn open_post_persist(
        &self,
        snapshot: Arc<PersistenceDataCache>,
        block: Arc<Block>,
        seeded: &SeededNativeContracts,
        cache: &NativeCacheHandle,
    ) -> Result<Box<dyn NativeEnginePort>, LedgerError> {
        let engine = ApplicationEngine::new_with_preloaded_native_shared(
            TriggerType::PostPersist,
            None,
            snapshot,
            Some(self.payload_block(&block)),
            self.settings.clone(),
            TEST_MODE_GAS,
            seeded.clone(),
            Arc::clone(cache),
            None,
            self.native_services.clone(),
        )
        .map_err(engine_error)?;
        Ok(Box::new(self.engine_port(engine)))
    }
}

#[derive(Clone)]
struct VerificationContext {
    container: Arc<dyn Verifiable>,
    snapshot: Arc<PersistenceDataCache>,
    payload_block: Arc<neo_protocol::payloads::Block>,
    system_fee: i64,
    seeded: SeededNativeContracts,
    cache: NativeCacheHandle,
    settings: neo_protocol::ProtocolSettings,
    native_services: EngineNativeServices,
}

impl CanonicalVerificationSource for VerificationContext {
    fn execute_canonical(&self, script: &[u8]) -> Result<ApplicationEngine, String> {
        let isolated = Arc::new(self.snapshot.fork_isolated());
        let mut engine = ApplicationEngine::new_with_preloaded_native_shared(
            TriggerType::Application,
            Some(Arc::clone(&self.container)),
            isolated,
            Some(Arc::clone(&self.payload_block)),
            self.settings.clone(),
            self.system_fee,
            self.seeded.clone(),
            Arc::clone(&self.cache),
            None,
            self.native_services.clone(),
        )
        .map_err(|error| error.to_string())?;
        engine
            .load_script(script.to_vec(), CallFlags::ALL, None)
            .map_err(|error| error.to_string())?;
        engine.execute_allow_fault();
        Ok(engine)
    }
}

struct SystemEnginePort {
    engine: ApplicationEngine,
    recorder: Arc<ExecutionRecorder>,
    execution_mode: ExecutionMode,
    stats: Arc<ExecutionStats>,
    script: Option<Vec<u8>>,
    transfer: Option<TransferParams>,
    verification: Option<VerificationContext>,
    verification_error: Option<String>,
    /// Observation record produced by a speculative run and held back until the
    /// ordered validation decides the transaction's position in commit order.
    pending_capture: Option<ApplicationExecuted>,
}

impl SystemEnginePort {
    fn take_transaction_states(&mut self) -> LedgerTransactionStates {
        self.engine
            .take_state::<LedgerTransactionStates>()
            .unwrap_or_else(|| LedgerTransactionStates::new(Vec::new()))
    }

    /// Delegates the execution-strategy decision to the smart-contract layer
    /// and retains only the failure surfaced by dual execution.
    fn execute_application(&mut self) -> VmState {
        let request = ExecutionRequest {
            mode: self.execution_mode,
            transfer: self.transfer.as_ref(),
            script: self.script.as_deref(),
            canonical_source: self
                .verification
                .as_ref()
                .map(|source| source as &dyn CanonicalVerificationSource),
            observer: Some(self.stats.as_ref() as &dyn ExecutionStrategyObserver),
        };
        let ExecutionReport {
            state,
            strategy,
            verification_error,
        } = execute_application_script(&mut self.engine, &request);
        self.stats
            .record_strategy_outcome(strategy, state, verification_error.as_deref());
        if let Some(message) = verification_error {
            self.verification_error = Some(message);
        }
        state
    }
}

impl NativeEnginePort for SystemEnginePort {
    fn native_on_persist(&mut self) -> Result<(), LedgerError> {
        self.engine.native_on_persist().map_err(engine_error)
    }

    fn native_post_persist(&mut self) -> Result<(), LedgerError> {
        self.engine.native_post_persist().map_err(engine_error)
    }

    fn record_native_persist_success(&mut self) {
        self.engine.record_native_persist_success();
    }

    fn set_transaction_states(&mut self, states: LedgerTransactionStates) {
        self.engine.set_state(states);
    }

    fn take_transaction_states(&mut self) -> LedgerTransactionStates {
        SystemEnginePort::take_transaction_states(self)
    }

    fn seed_native_contracts(&self) -> SeededNativeContracts {
        let native_hashes: HashSet<_> = self
            .engine
            .native_contracts()
            .into_iter()
            .map(|contract| contract.hash())
            .collect();
        let mut seeded = self.engine.contracts_shared_snapshot();
        seeded.retain(|hash, _| native_hashes.contains(hash));
        seeded
    }

    fn native_cache_handle(&self) -> NativeCacheHandle {
        self.engine.native_contract_cache_handle()
    }

    fn execution_receipt(&mut self) -> ExecutionReceipt {
        self.recorder.capture(&mut self.engine)
    }
}

impl ApplicationEnginePort for SystemEnginePort {
    fn load_script(&mut self, script: Vec<u8>) -> Result<(), LedgerError> {
        self.transfer = parse_fast_path_candidate(&script);
        self.script = Some(script.clone());
        self.verification_error = None;
        self.engine
            .load_script(script, CallFlags::ALL, None)
            .map_err(engine_error)
    }

    fn execute_allow_fault(&mut self) -> VmState {
        self.execute_application()
    }

    fn verify_execution(&self) -> Result<(), LedgerError> {
        match &self.verification_error {
            Some(message) => Err(LedgerError::Engine {
                message: message.clone(),
            }),
            None => Ok(()),
        }
    }

    fn record_transaction_vm_state(&mut self, transaction_hash: &UInt256, vm_state: VmState) {
        self.engine
            .record_transaction_vm_state(transaction_hash, vm_state);
    }

    fn set_transaction_states(&mut self, states: LedgerTransactionStates) {
        self.engine.set_state(states);
    }

    fn take_transaction_states(&mut self) -> LedgerTransactionStates {
        SystemEnginePort::take_transaction_states(self)
    }

    fn fee_consumed(&self) -> i64 {
        self.engine.fee_consumed()
    }

    fn fault_exception(&self) -> Option<String> {
        self.engine.fault_exception().map(str::to_owned)
    }

    fn fault_unknown_hardfork_name(&self) -> Option<String> {
        self.engine.fault_unknown_hardfork_name().map(str::to_owned)
    }

    fn execution_receipt(&mut self) -> ExecutionReceipt {
        self.recorder.capture(&mut self.engine)
    }

    fn enable_rw_set_tracking_at_generation(&mut self, generation: u64) {
        self.engine.enable_rw_set_tracking_at_generation(generation);
    }

    fn capture_speculative(&mut self, detailed: bool) -> Option<SpeculativeExecution> {
        let rw_set = self.engine.take_rw_set()?;
        let receipt = if detailed {
            let (executed, receipt) = self.recorder.project(&mut self.engine);
            self.pending_capture = Some(executed);
            receipt
        } else {
            ExecutionRecorder::receipt(&mut self.engine)
        };
        Some(SpeculativeExecution { receipt, rw_set })
    }

    fn publish_capture(&mut self) {
        if let Some(executed) = self.pending_capture.take() {
            self.recorder.publish(executed);
        }
    }
}
