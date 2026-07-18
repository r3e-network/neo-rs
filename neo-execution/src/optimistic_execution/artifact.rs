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
use neo_primitives::{TriggerType, UInt256, Verifiable};
use neo_storage::{CacheRead, Trackable};
use neo_vm::ExecutionEngineLimits;
use parking_lot::Mutex;
use std::sync::Arc;

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
}

/// Runner-owned observation binding for one optimistic execution.
#[derive(Clone)]
pub struct OptimisticObservationBinding {
    observations: Arc<Mutex<ExecutionObservationState>>,
    identity: Arc<Mutex<Option<SpeculativeExecutionIdentity>>>,
    limits: ExecutionArtifactLimits,
}

impl OptimisticObservationBinding {
    /// Creates a disabled-by-default binding. It becomes live only after
    /// [`Self::bind`] is explicitly called on one engine.
    #[must_use]
    pub fn new(limits: ExecutionArtifactLimits) -> Self {
        Self {
            observations: Arc::new(Mutex::new(ExecutionObservationState::new(limits))),
            identity: Arc::new(Mutex::new(None)),
            limits,
        }
    }

    /// Binds observations and immutable execution identity before script load.
    pub fn bind<P, D, B>(
        &self,
        engine: &mut ApplicationEngine<P, D, B>,
    ) -> Result<(), SpeculativeArtifactCaptureError>
    where
        P: NativeContractProvider + 'static,
        D: Diagnostic + 'static,
        B: CacheRead,
    {
        let mut identity_guard = self.identity.lock();
        if identity_guard.is_some() {
            return Err(SpeculativeArtifactCaptureError::BindingAlreadyConsumed);
        }
        if engine.execution_observation_handle().is_some() {
            return Err(SpeculativeArtifactCaptureError::EngineAlreadyBound);
        }
        let identity = capture_identity(engine)?;
        engine.bind_execution_observations(Arc::clone(&self.observations));
        *identity_guard = Some(identity);
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

    pub(crate) fn identity(
        &self,
    ) -> Result<SpeculativeExecutionIdentity, SpeculativeArtifactCaptureError> {
        self.identity
            .lock()
            .clone()
            .ok_or(SpeculativeArtifactCaptureError::BindingMissing)
    }

    pub(crate) fn host_dependencies(
        &self,
    ) -> Result<OptimisticHostDependencies, SpeculativeArtifactCaptureError> {
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
    pub fn record_native_cache(
        &self,
        access: crate::host_access_audit::NativeCacheAccess,
        before: Option<Vec<u8>>,
        after: Option<Vec<u8>>,
    ) -> Result<(), SpeculativeArtifactCaptureError> {
        if self.identity.lock().is_none() {
            return Err(SpeculativeArtifactCaptureError::BindingMissing);
        }
        let mut observations = self.observations.lock();
        observations.record(|journal| journal.record_native_cache(access, before, after));
        observations.journal().map(|_| ()).map_err(Into::into)
    }
}

fn capture_identity<P, D, B>(
    engine: &ApplicationEngine<P, D, B>,
) -> Result<SpeculativeExecutionIdentity, SpeculativeArtifactCaptureError>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: CacheRead,
{
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
    overlay: IsolatedTransactionOverlay<B>,
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
    /// Captures one completed engine and its detached effects for later
    /// ordered validation. No canonical artifact conversion is performed.
    pub fn capture(
        engine: ApplicationEngine<P, D, B>,
        overlay: IsolatedTransactionOverlay<B>,
        point_capture: TransactionDependencyCapture,
        observations: &OptimisticObservationBinding,
    ) -> Result<Self, SpeculativeArtifactCaptureError> {
        let engine_cache = engine.original_snapshot_cache_handle();
        let overlay_cache = overlay.snapshot_cache();
        if !Arc::ptr_eq(&engine_cache, &overlay_cache) {
            return Err(SpeculativeArtifactCaptureError::SnapshotMismatch);
        }
        if !overlay.owns_dependency_capture(&point_capture) {
            return Err(SpeculativeArtifactCaptureError::DependencyCaptureMismatch);
        }
        if !observations.is_bound_to(&engine) {
            return Err(SpeculativeArtifactCaptureError::BindingMissing);
        }

        let captured = (|| {
            let identity = observations.identity()?;
            let point_dependencies = point_capture.snapshot()?;
            let host_dependencies = observations.host_dependencies()?;
            let storage_effects = capture_storage_effects(&overlay, observations.limits())?;
            Ok::<_, SpeculativeArtifactCaptureError>((
                identity,
                point_dependencies,
                host_dependencies,
                storage_effects,
            ))
        })();
        engine_cache.disable_read_observation();
        let (identity, point_dependencies, host_dependencies, storage_effects) = captured?;

        Ok(Self {
            identity,
            prefix: overlay.prefix(),
            transaction_index: overlay.transaction_index(),
            engine,
            overlay,
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

    /// Borrow the canonical NeoVM engine state retained by the artifact.
    #[must_use]
    pub fn engine(&self) -> &ApplicationEngine<P, D, B> {
        &self.engine
    }

    /// Borrow the detached overlay retained by the artifact.
    #[must_use]
    pub fn overlay(&self) -> &IsolatedTransactionOverlay<B> {
        &self.overlay
    }

    /// Consume the artifact and return its canonical engine without publishing
    /// any effects.
    #[must_use]
    pub fn into_engine(self) -> ApplicationEngine<P, D, B> {
        self.engine
    }
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
        let binding = OptimisticObservationBinding::new(ExecutionArtifactLimits::default());
        binding.bind(&mut engine).expect("binding");
        let identity = binding.identity().expect("identity");
        let mut changed = identity.clone();
        changed.current_block_index = changed.current_block_index.saturating_add(1);
        changed.fee_limit_pico = changed.fee_limit_pico.saturating_add(1);
        assert_eq!(
            identity.first_mismatch(&changed),
            Some(SpeculativeIdentityComponent::CurrentBlockIndex)
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
        let cache = overlay.snapshot_cache();
        let mut engine = make_engine(cache);
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
        overlay
            .snapshot_cache()
            .update(key.clone(), StorageItem::from_bytes(b"changed".to_vec()));
        let artifact =
            SpeculativeExecutionArtifact::capture(engine, overlay, point_capture, &binding)
                .expect("artifact");
        assert_eq!(artifact.point_dependencies().point_reads().len(), 1);
        assert_eq!(artifact.host_dependencies().contexts().len(), 1);
        assert_eq!(artifact.host_dependencies().native_effects().len(), 1);
        assert_eq!(artifact.storage_effects().len(), 1);
        assert_eq!(
            artifact.storage_effects()[0].key(),
            &key,
            "artifact retains the canonical storage key"
        );
    }
}
