use super::{
    HostDependencyCaptureError, IsolatedTransactionOverlay, OptimisticHostDependencies,
    TransactionDependencies, TransactionDependencyCapture,
};
use crate::application_engine::ApplicationEngine;
use crate::diagnostic::Diagnostic;
use crate::execution_artifact::{
    ExecutionArtifactError, ExecutionArtifactLimits, ExecutionObservationState,
};
use crate::native_contract_provider::NativeContractProvider;
use neo_config::ProtocolSettings;
use neo_primitives::{TriggerType, UInt160, UInt256, Verifiable};
use neo_storage::{CacheRead, DataCache, Trackable};
use neo_vm::ExecutionEngineLimits;
use parking_lot::Mutex;
use std::sync::Arc;

/// Exact entry script selected immediately before speculative execution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpeculativeEntryScriptIdentity {
    logical_hash: UInt160,
    bytecode: Arc<[u8]>,
    entry_instruction_pointer: usize,
    return_value_count: i32,
    call_flags: u8,
}

impl SpeculativeEntryScriptIdentity {
    /// Logical script hash visible through the application host.
    #[must_use]
    pub const fn logical_hash(&self) -> UInt160 {
        self.logical_hash
    }

    /// Exact NeoVM bytecode selected for the entry context.
    #[must_use]
    pub fn bytecode(&self) -> &[u8] {
        self.bytecode.as_ref()
    }

    /// Instruction pointer at which entry execution starts.
    #[must_use]
    pub const fn entry_instruction_pointer(&self) -> usize {
        self.entry_instruction_pointer
    }

    /// NeoVM return-value count configured for the entry context.
    #[must_use]
    pub const fn return_value_count(&self) -> i32 {
        self.return_value_count
    }

    /// Exact entry-context call flags.
    #[must_use]
    pub const fn call_flags(&self) -> u8 {
        self.call_flags
    }
}

/// Immutable execution inputs that must match before a speculative artifact
/// can be considered for ordered application.
#[derive(Clone, Debug, PartialEq)]
pub struct SpeculativeExecutionIdentity {
    trigger: TriggerType,
    protocol: Arc<ProtocolSettings>,
    vm_limits: ExecutionEngineLimits,
    current_block_index: u32,
    persisting_block_hash: Option<UInt256>,
    persisting_block_timestamp: Option<u64>,
    script_container_hash: Option<UInt256>,
    fee_limit_pico: i64,
    exec_fee_factor: u32,
    storage_price: u32,
    nonce_data: [u8; 16],
    entry_script: SpeculativeEntryScriptIdentity,
}

impl SpeculativeExecutionIdentity {
    /// Trigger used by the execution.
    #[must_use]
    pub const fn trigger(&self) -> TriggerType {
        self.trigger
    }

    /// Protocol settings identity used by the execution.
    #[must_use]
    pub fn protocol(&self) -> &ProtocolSettings {
        self.protocol.as_ref()
    }

    /// NeoVM limits used by the execution.
    #[must_use]
    pub const fn vm_limits(&self) -> ExecutionEngineLimits {
        self.vm_limits
    }

    /// Current Ledger index used by the execution.
    #[must_use]
    pub const fn current_block_index(&self) -> u32 {
        self.current_block_index
    }

    /// Persisting block hash, when this is a block execution.
    #[must_use]
    pub const fn persisting_block_hash(&self) -> Option<UInt256> {
        self.persisting_block_hash
    }

    /// Persisting block timestamp, when this is a block execution.
    #[must_use]
    pub const fn persisting_block_timestamp(&self) -> Option<u64> {
        self.persisting_block_timestamp
    }

    /// Script-container hash used by the transaction.
    #[must_use]
    pub const fn script_container_hash(&self) -> Option<UInt256> {
        self.script_container_hash
    }

    /// Raw execution fee limit.
    #[must_use]
    pub const fn fee_limit_pico(&self) -> i64 {
        self.fee_limit_pico
    }

    /// Policy execution fee factor captured before execution.
    #[must_use]
    pub const fn exec_fee_factor(&self) -> u32 {
        self.exec_fee_factor
    }

    /// Policy storage price captured before execution.
    #[must_use]
    pub const fn storage_price(&self) -> u32 {
        self.storage_price
    }

    /// Initial deterministic nonce input.
    #[must_use]
    pub const fn nonce_data(&self) -> [u8; 16] {
        self.nonce_data
    }

    /// Exact entry script and entry position selected for execution.
    #[must_use]
    pub const fn entry_script(&self) -> &SpeculativeEntryScriptIdentity {
        &self.entry_script
    }
}

/// First identity field that differs between speculative and canonical inputs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpeculativeIdentityComponent {
    /// Trigger type differs.
    Trigger,
    /// Protocol settings differ.
    Protocol,
    /// NeoVM execution limits differ.
    VmLimits,
    /// Current block index differs.
    CurrentBlockIndex,
    /// Persisting block hash differs.
    PersistingBlockHash,
    /// Persisting block timestamp differs.
    PersistingBlockTimestamp,
    /// Script-container hash differs.
    ScriptContainerHash,
    /// Fee limit differs.
    FeeLimit,
    /// Policy execution fee factor differs.
    ExecFeeFactor,
    /// Policy storage price differs.
    StoragePrice,
    /// Initial nonce input differs.
    NonceData,
    /// Exact entry script bytes, logical hash, or entry position differ.
    EntryScript,
}

impl SpeculativeExecutionIdentity {
    /// Returns the first deterministic identity mismatch, if any.
    #[must_use]
    pub fn first_mismatch(&self, other: &Self) -> Option<SpeculativeIdentityComponent> {
        let checks = [
            (
                self.trigger != other.trigger,
                SpeculativeIdentityComponent::Trigger,
            ),
            (
                self.protocol != other.protocol,
                SpeculativeIdentityComponent::Protocol,
            ),
            (
                self.vm_limits != other.vm_limits,
                SpeculativeIdentityComponent::VmLimits,
            ),
            (
                self.current_block_index != other.current_block_index,
                SpeculativeIdentityComponent::CurrentBlockIndex,
            ),
            (
                self.persisting_block_hash != other.persisting_block_hash,
                SpeculativeIdentityComponent::PersistingBlockHash,
            ),
            (
                self.persisting_block_timestamp != other.persisting_block_timestamp,
                SpeculativeIdentityComponent::PersistingBlockTimestamp,
            ),
            (
                self.script_container_hash != other.script_container_hash,
                SpeculativeIdentityComponent::ScriptContainerHash,
            ),
            (
                self.fee_limit_pico != other.fee_limit_pico,
                SpeculativeIdentityComponent::FeeLimit,
            ),
            (
                self.exec_fee_factor != other.exec_fee_factor,
                SpeculativeIdentityComponent::ExecFeeFactor,
            ),
            (
                self.storage_price != other.storage_price,
                SpeculativeIdentityComponent::StoragePrice,
            ),
            (
                self.nonce_data != other.nonce_data,
                SpeculativeIdentityComponent::NonceData,
            ),
            (
                self.entry_script != other.entry_script,
                SpeculativeIdentityComponent::EntryScript,
            ),
        ];
        checks
            .into_iter()
            .find_map(|(different, component)| different.then_some(component))
    }
}

/// Failure while binding or materializing a speculative execution artifact.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SpeculativeArtifactCaptureError {
    /// An engine already has another observation binding.
    #[error("application engine already has an observation binding")]
    EngineAlreadyBound,
    /// This binding was already consumed for another engine.
    #[error("optimistic observation binding was already consumed")]
    BindingAlreadyConsumed,
    /// This binding has completed capture and cannot record more observations.
    #[error("optimistic observation binding is sealed")]
    BindingSealed,
    /// The binding was not installed before artifact capture.
    #[error("optimistic observation binding is missing")]
    BindingMissing,
    /// Engine and detached overlay do not share the exact cache handle.
    #[error("speculative engine and detached overlay cache handles differ")]
    SnapshotMismatch,
    /// Point dependency capture belongs to another detached overlay.
    #[error("point dependency capture does not belong to the detached overlay")]
    DependencyCaptureMismatch,
    /// A block or container hash could not be computed.
    #[error("unable to capture speculative identity hash for {kind}")]
    IdentityHash {
        /// Identity field whose hash failed.
        kind: &'static str,
    },
    /// Speculative execution must start with exactly one loaded entry context.
    #[error("speculative execution requires exactly one entry context, found {actual}")]
    EntryContextCount {
        /// Loaded invocation contexts at the capture boundary.
        actual: usize,
    },
    /// The loaded entry context has no logical application script hash.
    #[error("speculative entry context has no logical script hash")]
    EntryScriptHashMissing,
    /// Transaction speculation currently requires an empty initial VM stack.
    #[error("speculative entry context starts with {actual} evaluation-stack items")]
    EntryEvaluationStackNotEmpty {
        /// Evaluation-stack depth before the first instruction executes.
        actual: usize,
    },
    /// Live observations failed or exceeded their bound.
    #[error("speculative observation capture failed: {0}")]
    Observation(#[from] ExecutionArtifactError),
    /// Context/native dependency conversion found an inconsistent trace.
    #[error("speculative host dependency capture failed: {0}")]
    HostDependency(#[from] HostDependencyCaptureError),
    /// Point-read capture was unsupported or exceeded its bounds.
    #[error("speculative point dependency capture failed: {0}")]
    PointDependency(#[from] super::DependencyCaptureError),
    /// Detached storage effects exceeded the configured count bound.
    #[error("speculative storage effects require {actual}, maximum {maximum}")]
    StorageEffectLimit {
        /// Number of detached effects observed.
        actual: usize,
        /// Configured maximum effect count.
        maximum: usize,
    },
    /// Detached storage effects exceeded the configured byte bound.
    #[error("speculative storage effect bytes require {actual}, maximum {maximum}")]
    StorageEffectBytes {
        /// Serialized bytes required by detached effects.
        actual: usize,
        /// Configured maximum effect bytes.
        maximum: usize,
    },
    /// The runner received an overlay that had already been mutated before
    /// speculative execution began.
    #[error("speculative execution overlay already contains {actual} storage effects")]
    PreexistingStorageEffects {
        /// Number of tracked mutations present before the first VM instruction.
        actual: usize,
    },
    /// The runner received an engine whose observable execution state was not
    /// pristine before the first VM instruction.
    #[error("speculative execution engine has pre-existing {component}")]
    PreexistingEngineEffects {
        /// First non-pristine engine component in deterministic check order.
        component: &'static str,
    },
}

/// Runner-owned observation binding for one optimistic execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ObservationBindingPhase {
    Fresh,
    Bound,
    Executing,
    Sealed,
}

/// Runner-owned observation binding for one optimistic execution.
#[derive(Clone)]
pub struct OptimisticObservationBinding {
    observations: Arc<Mutex<ExecutionObservationState>>,
    phase: Arc<Mutex<ObservationBindingPhase>>,
    limits: ExecutionArtifactLimits,
}

impl OptimisticObservationBinding {
    /// Creates a disabled-by-default binding. It becomes live only after
    /// [`Self::bind`] is explicitly called on one engine.
    #[must_use]
    pub(crate) fn new(limits: ExecutionArtifactLimits) -> Self {
        Self {
            observations: Arc::new(Mutex::new(ExecutionObservationState::new(limits))),
            phase: Arc::new(Mutex::new(ObservationBindingPhase::Fresh)),
            limits,
        }
    }

    /// Binds live observations before script load. Exact execution identity is
    /// sealed immediately before [`SpeculativeExecutionArtifact::execute_and_capture`]
    /// enters NeoVM.
    pub(crate) fn bind<P, D, B>(
        &self,
        engine: &mut ApplicationEngine<P, D, B>,
    ) -> Result<(), SpeculativeArtifactCaptureError>
    where
        P: NativeContractProvider + 'static,
        D: Diagnostic + 'static,
        B: CacheRead,
    {
        let mut phase = self.phase.lock();
        match *phase {
            ObservationBindingPhase::Fresh => {}
            ObservationBindingPhase::Sealed => {
                return Err(SpeculativeArtifactCaptureError::BindingSealed);
            }
            ObservationBindingPhase::Bound | ObservationBindingPhase::Executing => {
                return Err(SpeculativeArtifactCaptureError::BindingAlreadyConsumed);
            }
        }
        if engine.execution_observation_handle().is_some() {
            return Err(SpeculativeArtifactCaptureError::EngineAlreadyBound);
        }
        engine.bind_execution_observations(Arc::clone(&self.observations));
        *phase = ObservationBindingPhase::Bound;
        Ok(())
    }

    pub(crate) fn is_bound_to<P, D, B>(&self, engine: &ApplicationEngine<P, D, B>) -> bool
    where
        P: NativeContractProvider + 'static,
        D: Diagnostic + 'static,
        B: CacheRead,
    {
        engine
            .execution_observation_handle()
            .is_some_and(|observations| Arc::ptr_eq(&observations, &self.observations))
    }

    fn begin_execution<P, D, B>(
        &self,
        engine: &ApplicationEngine<P, D, B>,
    ) -> Result<SpeculativeExecutionIdentity, SpeculativeArtifactCaptureError>
    where
        P: NativeContractProvider + 'static,
        D: Diagnostic + 'static,
        B: CacheRead,
    {
        if !self.is_bound_to(engine) {
            return Err(SpeculativeArtifactCaptureError::BindingMissing);
        }
        let mut phase = self.phase.lock();
        match *phase {
            ObservationBindingPhase::Fresh => {
                return Err(SpeculativeArtifactCaptureError::BindingMissing);
            }
            ObservationBindingPhase::Bound => {}
            ObservationBindingPhase::Executing => {
                return Err(SpeculativeArtifactCaptureError::BindingAlreadyConsumed);
            }
            ObservationBindingPhase::Sealed => {
                return Err(SpeculativeArtifactCaptureError::BindingSealed);
            }
        }
        let identity = capture_identity(engine)?;
        *phase = ObservationBindingPhase::Executing;
        Ok(identity)
    }

    fn seal(&self) {
        *self.phase.lock() = ObservationBindingPhase::Sealed;
    }

    fn seal_and_host_dependencies(
        &self,
    ) -> Result<OptimisticHostDependencies, SpeculativeArtifactCaptureError> {
        let mut phase = self.phase.lock();
        if *phase != ObservationBindingPhase::Executing {
            return Err(match *phase {
                ObservationBindingPhase::Fresh => SpeculativeArtifactCaptureError::BindingMissing,
                ObservationBindingPhase::Bound => {
                    SpeculativeArtifactCaptureError::BindingAlreadyConsumed
                }
                ObservationBindingPhase::Executing => unreachable!(),
                ObservationBindingPhase::Sealed => SpeculativeArtifactCaptureError::BindingSealed,
            });
        }
        *phase = ObservationBindingPhase::Sealed;
        let observations = self.observations.lock();
        let journal = observations.journal()?;
        Ok(OptimisticHostDependencies::from_journal(journal)?)
    }

    pub(crate) const fn limits(&self) -> ExecutionArtifactLimits {
        self.limits
    }

    /// Records one native-cache read/write observation for an explicitly bound
    /// optimistic adapter. The adapter remains responsible for performing the
    /// actual native-cache access and supplying its before/after bytes.
    pub(crate) fn record_native_cache(
        &self,
        access: crate::host_access_audit::NativeCacheAccess,
        before: Option<Vec<u8>>,
        after: Option<Vec<u8>>,
    ) -> Result<(), SpeculativeArtifactCaptureError> {
        let phase = self.phase.lock();
        match *phase {
            ObservationBindingPhase::Fresh => {
                return Err(SpeculativeArtifactCaptureError::BindingMissing);
            }
            ObservationBindingPhase::Sealed => {
                return Err(SpeculativeArtifactCaptureError::BindingSealed);
            }
            ObservationBindingPhase::Bound | ObservationBindingPhase::Executing => {}
        }
        let mut observations = self.observations.lock();
        observations.record(|journal| journal.record_native_cache(access, before, after));
        observations.journal().map(|_| ()).map_err(Into::into)
    }
}

/// Fail-closed cleanup for every capture exit, including unwinding VM panics.
///
/// The runner owns both cache handles and the binding for the duration of
/// execution. Dropping this guard permanently disables point observation and
/// seals host observation. Cleanup also removes the observation handle from an
/// engine returned in the artifact.
struct OptimisticCaptureRunner<'a, P, D, B>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: CacheRead,
{
    engine: Option<ApplicationEngine<P, D, B>>,
    engine_cache: Arc<DataCache<B>>,
    overlay_cache: Arc<DataCache<B>>,
    point_capture: TransactionDependencyCapture,
    binding: &'a OptimisticObservationBinding,
}

impl<'a, P, D, B> OptimisticCaptureRunner<'a, P, D, B>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: CacheRead,
{
    fn new(
        engine: ApplicationEngine<P, D, B>,
        engine_cache: Arc<DataCache<B>>,
        overlay_cache: Arc<DataCache<B>>,
        point_capture: TransactionDependencyCapture,
        binding: &'a OptimisticObservationBinding,
    ) -> Self {
        Self {
            engine: Some(engine),
            engine_cache,
            overlay_cache,
            point_capture,
            binding,
        }
    }

    fn engine(&self) -> &ApplicationEngine<P, D, B> {
        self.engine.as_ref().expect("capture runner owns engine")
    }

    fn engine_mut(&mut self) -> &mut ApplicationEngine<P, D, B> {
        self.engine.as_mut().expect("capture runner owns engine")
    }

    fn point_capture(&self) -> &TransactionDependencyCapture {
        &self.point_capture
    }

    fn disable_point_observation(&self) {
        self.engine_cache.disable_read_observation();
        if !Arc::ptr_eq(&self.engine_cache, &self.overlay_cache) {
            self.overlay_cache.disable_read_observation();
        }
        self.point_capture.seal();
    }

    fn cleanup(&mut self) {
        self.disable_point_observation();
        if let Some(engine) = self.engine.as_mut() {
            let _ = engine.unbind_execution_observations(&self.binding.observations);
        }
        self.binding.seal();
    }

    fn into_engine(mut self) -> ApplicationEngine<P, D, B> {
        self.cleanup();
        self.engine.take().expect("capture runner owns engine")
    }
}

impl<P, D, B> Drop for OptimisticCaptureRunner<'_, P, D, B>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: CacheRead,
{
    fn drop(&mut self) {
        self.cleanup();
    }
}

pub(super) fn capture_identity<P, D, B>(
    engine: &ApplicationEngine<P, D, B>,
) -> Result<SpeculativeExecutionIdentity, SpeculativeArtifactCaptureError>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: CacheRead,
{
    let invocation_stack = engine.invocation_stack();
    if invocation_stack.len() != 1 {
        return Err(SpeculativeArtifactCaptureError::EntryContextCount {
            actual: invocation_stack.len(),
        });
    }
    let entry_context = &invocation_stack[0];
    let initial_stack_depth = entry_context.evaluation_stack().len();
    if initial_stack_depth != 0 {
        return Err(
            SpeculativeArtifactCaptureError::EntryEvaluationStackNotEmpty {
                actual: initial_stack_depth,
            },
        );
    }
    let entry_state = entry_context.state();
    let entry_state = entry_state.lock();
    let logical_hash = entry_state
        .script_hash
        .ok_or(SpeculativeArtifactCaptureError::EntryScriptHashMissing)?;
    let call_flags = entry_state.call_flags.bits();
    drop(entry_state);
    let persisting_block_hash = engine
        .persisting_block()
        .map(|block| {
            block
                .try_hash()
                .map_err(|_| SpeculativeArtifactCaptureError::IdentityHash {
                    kind: "persisting block",
                })
        })
        .transpose()?;
    let script_container_hash = engine
        .script_container()
        .map(|container| {
            container
                .hash()
                .map_err(|_| SpeculativeArtifactCaptureError::IdentityHash {
                    kind: "script container",
                })
        })
        .transpose()?;
    Ok(SpeculativeExecutionIdentity {
        trigger: engine.trigger(),
        protocol: Arc::new(engine.protocol_settings().clone()),
        vm_limits: *engine.execution_limits(),
        current_block_index: engine.current_block_index(),
        persisting_block_hash,
        persisting_block_timestamp: engine
            .persisting_block()
            .map(|block| block.header.timestamp()),
        script_container_hash,
        fee_limit_pico: engine.fee_amount_pico(),
        exec_fee_factor: engine.exec_fee_factor_raw(),
        storage_price: engine.storage_price(),
        nonce_data: engine.nonce_data(),
        entry_script: SpeculativeEntryScriptIdentity {
            logical_hash,
            bytecode: entry_context.script().shared_bytes(),
            entry_instruction_pointer: entry_context.instruction_pointer(),
            return_value_count: entry_context.rvcount(),
            call_flags,
        },
    })
}

/// One detached storage effect retained in the canonical cache representation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpeculativeStorageEffect {
    key: neo_storage::StorageKey,
    trackable: Trackable,
}

impl SpeculativeStorageEffect {
    /// Exact storage key.
    #[must_use]
    pub const fn key(&self) -> &neo_storage::StorageKey {
        &self.key
    }

    /// Existing execution-layer trackable value and state.
    #[must_use]
    pub const fn trackable(&self) -> &Trackable {
        &self.trackable
    }
}

/// A speculative artifact that retains canonical engine state and detached
/// effects without converting VM values into a second representation.
pub struct SpeculativeExecutionArtifact<P, D, B = neo_storage::EmptyCacheBacking>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: CacheRead,
{
    identity: SpeculativeExecutionIdentity,
    prefix: super::BlockPrefixIdentity,
    transaction_index: usize,
    engine: ApplicationEngine<P, D, B>,
    point_dependencies: TransactionDependencies,
    host_dependencies: OptimisticHostDependencies,
    storage_effects: Vec<SpeculativeStorageEffect>,
}

impl<P, D, B> SpeculativeExecutionArtifact<P, D, B>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: CacheRead,
{
    /// Executes one prepared engine and immediately seals its detached effects.
    ///
    /// The engine must already contain its entry script. This method owns the
    /// engine across execution and capture, so no caller can mutate it between
    /// the terminal NeoVM result and the immutable artifact snapshot.
    pub(crate) fn execute_and_capture(
        engine: ApplicationEngine<P, D, B>,
        overlay: IsolatedTransactionOverlay<B>,
        point_capture: TransactionDependencyCapture,
        observations: &OptimisticObservationBinding,
    ) -> Result<Self, SpeculativeArtifactCaptureError> {
        let engine_cache = engine.original_snapshot_cache_handle();
        let overlay_cache = overlay.snapshot_cache();
        let mut runner = OptimisticCaptureRunner::new(
            engine,
            Arc::clone(&engine_cache),
            Arc::clone(&overlay_cache),
            point_capture,
            observations,
        );

        let captured = (|| {
            if !Arc::ptr_eq(&engine_cache, &overlay_cache) {
                return Err(SpeculativeArtifactCaptureError::SnapshotMismatch);
            }
            if !overlay.owns_dependency_capture(runner.point_capture()) {
                return Err(SpeculativeArtifactCaptureError::DependencyCaptureMismatch);
            }
            if !observations.is_bound_to(runner.engine()) {
                return Err(SpeculativeArtifactCaptureError::BindingMissing);
            }
            let preexisting_storage_effects = overlay.pending_storage_effects();
            if preexisting_storage_effects != 0 {
                return Err(SpeculativeArtifactCaptureError::PreexistingStorageEffects {
                    actual: preexisting_storage_effects,
                });
            }
            let identity = observations.begin_execution(runner.engine())?;
            ensure_pristine_engine(runner.engine(), &identity)?;
            runner.engine_mut().execute_allow_fault();
            // Disable the shared point observer before copying the dependency
            // set. `disable_read_observation` waits for in-flight callbacks, so
            // no retained cache handle can race an omitted read into the seal.
            runner.disable_point_observation();
            let point_dependencies = runner.point_capture().snapshot()?;
            let host_dependencies = observations.seal_and_host_dependencies()?;
            let storage_effects = capture_storage_effects(&overlay, observations.limits())?;
            Ok::<_, SpeculativeArtifactCaptureError>((
                identity,
                point_dependencies,
                host_dependencies,
                storage_effects,
            ))
        })();
        let (identity, point_dependencies, host_dependencies, storage_effects) = captured?;
        let engine = runner.into_engine();

        Ok(Self {
            identity,
            prefix: overlay.prefix(),
            transaction_index: overlay.transaction_index(),
            engine,
            point_dependencies,
            host_dependencies,
            storage_effects,
        })
    }

    /// Immutable execution identity.
    #[must_use]
    pub const fn identity(&self) -> &SpeculativeExecutionIdentity {
        &self.identity
    }

    /// Pinned block prefix identity.
    #[must_use]
    pub const fn prefix(&self) -> super::BlockPrefixIdentity {
        self.prefix
    }

    /// Canonical transaction position.
    #[must_use]
    pub const fn transaction_index(&self) -> usize {
        self.transaction_index
    }

    /// Exact point dependencies.
    #[must_use]
    pub const fn point_dependencies(&self) -> &TransactionDependencies {
        &self.point_dependencies
    }

    /// Context and native-cache dependencies/effects.
    #[must_use]
    pub const fn host_dependencies(&self) -> &OptimisticHostDependencies {
        &self.host_dependencies
    }

    /// Detached storage effects in canonical key order.
    #[must_use]
    pub fn storage_effects(&self) -> &[SpeculativeStorageEffect] {
        &self.storage_effects
    }

    pub(super) fn vm_state(&self) -> neo_vm::VmState {
        self.engine.state()
    }

    /// Consume the artifact and return its canonical engine without publishing
    /// any effects.
    #[must_use]
    pub fn into_engine(self) -> ApplicationEngine<P, D, B> {
        self.engine
    }

    /// Consumes the artifact for the ordered application boundary.
    ///
    /// Storage effects stay in the canonical [`neo_storage::Trackable`]
    /// representation. The detached overlay is dropped after its engine has
    /// been moved out; the engine retains its own cache handle and all VM
    /// results, diagnostics, notifications, logs, calls, and invocation
    /// counters.
    pub(super) fn into_application_parts(
        self,
    ) -> (
        ApplicationEngine<P, D, B>,
        TransactionDependencies,
        Vec<(neo_storage::StorageKey, Trackable)>,
    ) {
        let effects = self
            .storage_effects
            .into_iter()
            .map(|effect| (effect.key, effect.trackable))
            .collect();
        (self.engine, self.point_dependencies, effects)
    }
}

fn ensure_pristine_engine<P, D, B>(
    engine: &ApplicationEngine<P, D, B>,
    identity: &SpeculativeExecutionIdentity,
) -> Result<(), SpeculativeArtifactCaptureError>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: CacheRead,
{
    // Loading the entry script intentionally leaves NeoVM in BREAK. That is
    // the pristine pre-execution state; only a terminal/fault state (or any
    // execution-side counter/effect) must be rejected here.
    let component = if !matches!(
        engine.state(),
        neo_vm::VmState::NONE | neo_vm::VmState::BREAK
    ) {
        Some("VM state")
    } else if engine.instructions_executed() != 0 {
        Some("executed instructions")
    } else if engine.gas_consumed_pico() != 0 || engine.fee_consumed_pico() != 0 {
        Some("gas or fee consumption")
    } else if engine.fault_exception().is_some() || engine.uncaught_exception_item().is_some() {
        Some("fault state")
    } else if !engine.result_stack().is_empty() {
        Some("result stack")
    } else if !engine.notifications().is_empty() {
        Some("notifications")
    } else if !engine.logs().is_empty() {
        Some("logs")
    } else if engine.random_times() != 0 {
        Some("random counter")
    } else if engine.pending_native_call_count() != 0 {
        Some("pending native calls")
    } else if engine.storage_iterator_count() != 0 {
        Some("storage iterators")
    } else if engine.native_argument_null_mask() != 0 || engine.native_return_is_null() {
        Some("native-call scratch state")
    } else {
        let counters = engine.invocation_counters_snapshot();
        let expected_hash = identity.entry_script().logical_hash();
        (counters.len() != 1 || counters[0] != (expected_hash, 1)).then_some("invocation counters")
    };

    component.map_or(Ok(()), |component| {
        Err(SpeculativeArtifactCaptureError::PreexistingEngineEffects { component })
    })
}

fn capture_storage_effects<B: CacheRead>(
    overlay: &IsolatedTransactionOverlay<B>,
    limits: ExecutionArtifactLimits,
) -> Result<Vec<SpeculativeStorageEffect>, SpeculativeArtifactCaptureError> {
    let mut effects = Vec::new();
    let mut bytes = 0usize;
    overlay.visit_storage_effects(|key, trackable| {
        if effects.len() >= limits.max_storage_changes {
            return;
        }
        let value_bytes = trackable.item.value_bytes().len();
        bytes = bytes
            .saturating_add(key.length())
            .saturating_add(value_bytes);
        effects.push(SpeculativeStorageEffect {
            key: key.clone(),
            trackable: trackable.clone(),
        });
    });
    if effects.len() > limits.max_storage_changes {
        return Err(SpeculativeArtifactCaptureError::StorageEffectLimit {
            actual: effects.len(),
            maximum: limits.max_storage_changes,
        });
    }
    if effects.len() == limits.max_storage_changes {
        // `visit_storage_effects` cannot short-circuit its callback. Probe one
        // additional marker to distinguish exactly-at-limit from overflow.
        let mut count = 0usize;
        overlay.visit_storage_effects(|_, _| count = count.saturating_add(1));
        if count > limits.max_storage_changes {
            return Err(SpeculativeArtifactCaptureError::StorageEffectLimit {
                actual: count,
                maximum: limits.max_storage_changes,
            });
        }
    }
    if bytes > limits.max_retained_bytes {
        return Err(SpeculativeArtifactCaptureError::StorageEffectBytes {
            actual: bytes,
            maximum: limits.max_retained_bytes,
        });
    }
    Ok(effects)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application_engine::{ApplicationEngine, TEST_MODE_GAS};
    use crate::native_contract_provider::NoNativeContractProvider;
    use crate::{NoDiagnostic, TriggerType};
    use neo_config::ProtocolSettings;
    use neo_storage::{DataCache, StorageItem, StorageKey};

    const PANIC_SYSCALL: &str = "Test.OptimisticCapturePanic";

    fn panic_during_execution(
        _engine: &mut ApplicationEngine,
        _vm: &mut crate::ApplicationExecutionEngine,
    ) -> neo_vm::VmResult<()> {
        panic!("intentional optimistic capture panic");
    }

    fn panic_script() -> Vec<u8> {
        let mut script = vec![neo_vm::OpCode::SYSCALL.byte()];
        script.extend_from_slice(&neo_vm::interop_hash(PANIC_SYSCALL).to_le_bytes());
        script.push(neo_vm::OpCode::RET.byte());
        script
    }

    fn make_engine(
        cache: Arc<neo_storage::DataCache<neo_storage::EmptyCacheBacking>>,
    ) -> ApplicationEngine {
        ApplicationEngine::<NoNativeContractProvider>::new_with_shared_block_and_native_contract_provider(
            TriggerType::Application,
            None,
            cache,
            None,
            ProtocolSettings::default(),
            TEST_MODE_GAS,
            NoDiagnostic,
            Arc::new(NoNativeContractProvider),
        )
        .expect("engine")
    }

    #[test]
    fn identity_reports_first_stable_mismatch() {
        let cache = Arc::new(DataCache::new(false));
        let mut engine = make_engine(cache);
        engine
            .load_script(
                vec![neo_vm::OpCode::RET.byte()],
                neo_primitives::CallFlags::ALL,
                None,
            )
            .expect("entry script");
        let identity = capture_identity(&engine).expect("identity");
        let mut changed = identity.clone();
        changed.current_block_index = changed.current_block_index.saturating_add(1);
        changed.fee_limit_pico = changed.fee_limit_pico.saturating_add(1);
        assert_eq!(
            identity.first_mismatch(&changed),
            Some(SpeculativeIdentityComponent::CurrentBlockIndex)
        );

        let mut changed_entry = identity.clone();
        changed_entry.entry_script.call_flags ^= 1;
        assert_eq!(
            identity.first_mismatch(&changed_entry),
            Some(SpeculativeIdentityComponent::EntryScript)
        );
    }

    #[test]
    fn binding_rejects_second_engine_and_captures_host_observations() {
        let base = DataCache::new(false);
        let key = StorageKey::new(7, b"value".to_vec());
        base.add(key.clone(), StorageItem::from_bytes(b"prefix".to_vec()));
        let prefix = super::super::PinnedBlockPrefix::capture(
            super::super::BlockPrefixIdentity::new(neo_primitives::UInt256::default(), 1, 0),
            &base,
        );
        let (overlay, point_capture) = prefix
            .transaction_overlay_with_dependency_capture(
                0,
                super::super::DependencyCaptureLimits::default(),
            )
            .expect("overlay");
        let retained_point_capture = point_capture.clone();
        let retained_cache = overlay.snapshot_cache();
        let mut engine = make_engine(Arc::clone(&retained_cache));
        let binding = OptimisticObservationBinding::new(ExecutionArtifactLimits::default());
        binding.bind(&mut engine).expect("binding");
        engine.observe_context(
            crate::host_access_audit::HostContextAccess::Network,
            crate::execution_artifact::ContextObservationValue::U32(1),
        );
        binding
            .record_native_cache(
                crate::host_access_audit::NativeCacheAccess::new(
                    neo_vm::NativeCacheDomain {
                        contract_hash: neo_primitives::UInt160::zero(),
                        contract_id: 1,
                        native_version: 1,
                        partition: 1,
                    },
                    crate::host_access_audit::ResolvedNativeCacheScope::WholeDomain,
                    crate::host_access_audit::NativeCacheAccessKind::Write,
                ),
                None,
                Some(b"value".to_vec()),
            )
            .expect("native observation");
        overlay.snapshot_cache().get(&key);
        engine
            .load_script(
                vec![neo_vm::OpCode::RET.byte()],
                neo_primitives::CallFlags::ALL,
                None,
            )
            .expect("entry script");
        let artifact = SpeculativeExecutionArtifact::execute_and_capture(
            engine,
            overlay,
            point_capture,
            &binding,
        )
        .expect("artifact");
        assert_eq!(artifact.point_dependencies().point_reads().len(), 1);
        assert_eq!(artifact.host_dependencies().contexts().len(), 1);
        assert_eq!(artifact.host_dependencies().native_effects().len(), 1);
        assert!(artifact.storage_effects().is_empty());
        retained_cache.get(&StorageKey::new(7, b"post-capture".to_vec()));
        assert_eq!(
            retained_point_capture
                .snapshot()
                .expect("sealed point capture")
                .point_reads(),
            artifact.point_dependencies().point_reads()
        );
        assert_eq!(
            binding.record_native_cache(
                crate::host_access_audit::NativeCacheAccess::new(
                    neo_vm::NativeCacheDomain {
                        contract_hash: neo_primitives::UInt160::zero(),
                        contract_id: 1,
                        native_version: 1,
                        partition: 1,
                    },
                    crate::host_access_audit::ResolvedNativeCacheScope::WholeDomain,
                    crate::host_access_audit::NativeCacheAccessKind::Read,
                ),
                None,
                None,
            ),
            Err(SpeculativeArtifactCaptureError::BindingSealed)
        );
        assert!(artifact.engine.execution_observation_handle().is_none());
    }

    #[test]
    fn execute_and_capture_requires_a_loaded_entry_script() {
        let base = DataCache::new(false);
        let prefix = super::super::PinnedBlockPrefix::capture(
            super::super::BlockPrefixIdentity::new(neo_primitives::UInt256::default(), 1, 0),
            &base,
        );
        let (overlay, point_capture) = prefix
            .transaction_overlay_with_dependency_capture(
                0,
                super::super::DependencyCaptureLimits::default(),
            )
            .expect("overlay");
        let mut engine = make_engine(overlay.snapshot_cache());
        let binding = OptimisticObservationBinding::new(ExecutionArtifactLimits::default());
        binding.bind(&mut engine).expect("binding");

        let error = SpeculativeExecutionArtifact::execute_and_capture(
            engine,
            overlay,
            point_capture,
            &binding,
        )
        .err()
        .expect("missing entry script");
        assert_eq!(
            error,
            SpeculativeArtifactCaptureError::EntryContextCount { actual: 0 }
        );
        assert_eq!(
            binding.record_native_cache(
                crate::host_access_audit::NativeCacheAccess::new(
                    neo_vm::NativeCacheDomain {
                        contract_hash: neo_primitives::UInt160::zero(),
                        contract_id: 1,
                        native_version: 1,
                        partition: 1,
                    },
                    crate::host_access_audit::ResolvedNativeCacheScope::WholeDomain,
                    crate::host_access_audit::NativeCacheAccessKind::Read,
                ),
                None,
                None,
            ),
            Err(SpeculativeArtifactCaptureError::BindingSealed)
        );
    }

    #[test]
    fn preexisting_overlay_effects_fail_closed_before_vm_execution() {
        let base = DataCache::new(false);
        let key = StorageKey::new(7, b"preexisting".to_vec());
        let later_read = StorageKey::new(7, b"later-read".to_vec());
        let prefix = super::super::PinnedBlockPrefix::capture(
            super::super::BlockPrefixIdentity::new(neo_primitives::UInt256::default(), 1, 0),
            &base,
        );
        let (overlay, point_capture) = prefix
            .transaction_overlay_with_dependency_capture(
                0,
                super::super::DependencyCaptureLimits::default(),
            )
            .expect("overlay");
        let retained_capture = point_capture.clone();
        let retained_cache = overlay.snapshot_cache();
        retained_cache.add(key, StorageItem::from_bytes(b"forged".to_vec()));
        let mut engine = make_engine(overlay.snapshot_cache());
        let binding = OptimisticObservationBinding::new(ExecutionArtifactLimits::default());
        binding.bind(&mut engine).expect("binding");
        engine
            .load_script(
                vec![neo_vm::OpCode::RET.byte()],
                neo_primitives::CallFlags::ALL,
                None,
            )
            .expect("entry script");

        let error = SpeculativeExecutionArtifact::execute_and_capture(
            engine,
            overlay,
            point_capture,
            &binding,
        )
        .err()
        .expect("preexisting effect rejection");

        assert_eq!(
            error,
            SpeculativeArtifactCaptureError::PreexistingStorageEffects { actual: 1 }
        );
        retained_cache.get(&later_read);
        assert!(
            retained_capture
                .snapshot()
                .expect("sealed point capture")
                .point_reads()
                .is_empty()
        );
        assert_eq!(
            binding.record_native_cache(
                crate::host_access_audit::NativeCacheAccess::new(
                    neo_vm::NativeCacheDomain {
                        contract_hash: neo_primitives::UInt160::zero(),
                        contract_id: 1,
                        native_version: 1,
                        partition: 1,
                    },
                    crate::host_access_audit::ResolvedNativeCacheScope::WholeDomain,
                    crate::host_access_audit::NativeCacheAccessKind::Read,
                ),
                None,
                None,
            ),
            Err(SpeculativeArtifactCaptureError::BindingSealed)
        );
    }

    #[test]
    fn preexisting_engine_effects_fail_closed_before_vm_execution() {
        let base = DataCache::new(false);
        let prefix = super::super::PinnedBlockPrefix::capture(
            super::super::BlockPrefixIdentity::new(neo_primitives::UInt256::default(), 1, 0),
            &base,
        );
        let (overlay, point_capture) = prefix
            .transaction_overlay_with_dependency_capture(
                0,
                super::super::DependencyCaptureLimits::default(),
            )
            .expect("overlay");
        let mut engine = make_engine(overlay.snapshot_cache());
        let binding = OptimisticObservationBinding::new(ExecutionArtifactLimits::default());
        binding.bind(&mut engine).expect("binding");
        engine
            .load_script(
                vec![neo_vm::OpCode::RET.byte()],
                neo_primitives::CallFlags::ALL,
                None,
            )
            .expect("entry script");
        engine.set_fault_exception("forged pre-execution fault");

        let error = SpeculativeExecutionArtifact::execute_and_capture(
            engine,
            overlay,
            point_capture,
            &binding,
        )
        .err()
        .expect("preexisting engine effect rejection");

        assert_eq!(
            error,
            SpeculativeArtifactCaptureError::PreexistingEngineEffects {
                component: "fault state",
            }
        );
    }

    #[test]
    fn early_snapshot_mismatch_seals_binding_and_both_cache_observers() {
        let base = DataCache::new(false);
        let prefix = super::super::PinnedBlockPrefix::capture(
            super::super::BlockPrefixIdentity::new(neo_primitives::UInt256::default(), 1, 0),
            &base,
        );
        let (engine_overlay, engine_capture) = prefix
            .transaction_overlay_with_dependency_capture(
                0,
                super::super::DependencyCaptureLimits::default(),
            )
            .expect("engine overlay");
        let (other_overlay, other_capture) = prefix
            .transaction_overlay_with_dependency_capture(
                1,
                super::super::DependencyCaptureLimits::default(),
            )
            .expect("other overlay");
        let engine_cache = engine_overlay.snapshot_cache();
        let other_cache = other_overlay.snapshot_cache();
        let retained_engine_capture = engine_capture.clone();
        let retained_other_capture = other_capture.clone();
        let mut engine = make_engine(engine_overlay.snapshot_cache());
        let binding = OptimisticObservationBinding::new(ExecutionArtifactLimits::default());
        binding.bind(&mut engine).expect("binding");
        engine
            .load_script(
                vec![neo_vm::OpCode::RET.byte()],
                neo_primitives::CallFlags::ALL,
                None,
            )
            .expect("entry script");

        let error = SpeculativeExecutionArtifact::execute_and_capture(
            engine,
            other_overlay,
            other_capture,
            &binding,
        )
        .err()
        .expect("snapshot mismatch");

        assert_eq!(error, SpeculativeArtifactCaptureError::SnapshotMismatch);
        engine_cache.get(&StorageKey::new(7, b"engine-later".to_vec()));
        other_cache.get(&StorageKey::new(7, b"other-later".to_vec()));
        assert!(
            retained_engine_capture
                .snapshot()
                .expect("engine capture")
                .point_reads()
                .is_empty()
        );
        assert!(
            retained_other_capture
                .snapshot()
                .expect("other capture")
                .point_reads()
                .is_empty()
        );
        assert_eq!(
            binding.record_native_cache(
                crate::host_access_audit::NativeCacheAccess::new(
                    neo_vm::NativeCacheDomain {
                        contract_hash: neo_primitives::UInt160::zero(),
                        contract_id: 1,
                        native_version: 1,
                        partition: 1,
                    },
                    crate::host_access_audit::ResolvedNativeCacheScope::WholeDomain,
                    crate::host_access_audit::NativeCacheAccessKind::Read,
                ),
                None,
                None,
            ),
            Err(SpeculativeArtifactCaptureError::BindingSealed)
        );
    }

    #[test]
    fn dependency_capture_mismatch_seals_the_foreign_recorder() {
        let base = DataCache::new(false);
        let prefix = super::super::PinnedBlockPrefix::capture(
            super::super::BlockPrefixIdentity::new(neo_primitives::UInt256::default(), 1, 0),
            &base,
        );
        let (engine_overlay, engine_capture) = prefix
            .transaction_overlay_with_dependency_capture(
                0,
                super::super::DependencyCaptureLimits::default(),
            )
            .expect("engine overlay");
        let (foreign_overlay, foreign_capture) = prefix
            .transaction_overlay_with_dependency_capture(
                1,
                super::super::DependencyCaptureLimits::default(),
            )
            .expect("foreign overlay");
        let engine_cache = engine_overlay.snapshot_cache();
        let foreign_cache = foreign_overlay.snapshot_cache();
        let retained_engine_capture = engine_capture.clone();
        let retained_foreign_capture = foreign_capture.clone();
        let mut engine = make_engine(engine_overlay.snapshot_cache());
        let binding = OptimisticObservationBinding::new(ExecutionArtifactLimits::default());
        binding.bind(&mut engine).expect("binding");
        engine
            .load_script(
                vec![neo_vm::OpCode::RET.byte()],
                neo_primitives::CallFlags::ALL,
                None,
            )
            .expect("entry script");

        let error = SpeculativeExecutionArtifact::execute_and_capture(
            engine,
            engine_overlay,
            foreign_capture,
            &binding,
        )
        .err()
        .expect("dependency-capture mismatch");

        assert_eq!(
            error,
            SpeculativeArtifactCaptureError::DependencyCaptureMismatch
        );
        engine_cache.get(&StorageKey::new(7, b"engine-later".to_vec()));
        foreign_cache.get(&StorageKey::new(7, b"foreign-later".to_vec()));
        assert!(
            retained_engine_capture
                .snapshot()
                .expect("engine capture")
                .point_reads()
                .is_empty()
        );
        assert!(
            retained_foreign_capture
                .snapshot()
                .expect("foreign capture")
                .point_reads()
                .is_empty()
        );
    }

    #[test]
    fn execution_panic_unbinds_and_seals_all_observation_state() {
        let base = DataCache::new(false);
        let prefix = super::super::PinnedBlockPrefix::capture(
            super::super::BlockPrefixIdentity::new(neo_primitives::UInt256::default(), 1, 0),
            &base,
        );
        let (overlay, point_capture) = prefix
            .transaction_overlay_with_dependency_capture(
                0,
                super::super::DependencyCaptureLimits::default(),
            )
            .expect("overlay");
        let retained_capture = point_capture.clone();
        let retained_cache = overlay.snapshot_cache();
        let mut engine = make_engine(overlay.snapshot_cache());
        engine
            .register_host_service(
                PANIC_SYSCALL,
                0,
                neo_primitives::CallFlags::NONE,
                panic_during_execution,
            )
            .expect("panic syscall");
        let binding = OptimisticObservationBinding::new(ExecutionArtifactLimits::default());
        binding.bind(&mut engine).expect("binding");
        engine
            .load_script(panic_script(), neo_primitives::CallFlags::ALL, None)
            .expect("entry script");

        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = SpeculativeExecutionArtifact::execute_and_capture(
                engine,
                overlay,
                point_capture,
                &binding,
            );
        }));

        assert!(panic.is_err());
        retained_cache.get(&StorageKey::new(7, b"later".to_vec()));
        assert!(
            retained_capture
                .snapshot()
                .expect("sealed point capture")
                .point_reads()
                .is_empty()
        );
        assert_eq!(
            binding.record_native_cache(
                crate::host_access_audit::NativeCacheAccess::new(
                    neo_vm::NativeCacheDomain {
                        contract_hash: neo_primitives::UInt160::zero(),
                        contract_id: 1,
                        native_version: 1,
                        partition: 1,
                    },
                    crate::host_access_audit::ResolvedNativeCacheScope::WholeDomain,
                    crate::host_access_audit::NativeCacheAccessKind::Read,
                ),
                None,
                None,
            ),
            Err(SpeculativeArtifactCaptureError::BindingSealed)
        );
    }
}
