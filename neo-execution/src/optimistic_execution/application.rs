//! Ordered application of one validated speculative execution artifact.
//!
//! This is deliberately a foundation-only boundary. It can publish ordinary
//! storage deltas into a caller-owned canonical [`DataCache`], while the
//! original [`ApplicationEngine`] remains the authoritative VM result. Native
//! cache publication is not modeled here and therefore fails closed.

use super::artifact::capture_identity;
use super::{
    BlockPrefixIdentity, HostDependencyValidation, OptimisticContextValue, PointReadValidation,
    SpeculativeArtifactCaptureError, SpeculativeExecutionArtifact, SpeculativeExecutionIdentity,
    SpeculativeIdentityComponent,
};
use crate::application_engine::ApplicationEngine;
use crate::diagnostic::Diagnostic;
use crate::host_access_audit::HostContextAccess;
use crate::native_contract_provider::NativeContractProvider;
use neo_primitives::UInt256;
use neo_storage::{
    CacheRead, DataCache, DataCacheAtomicMergeError, DataCacheError, DataCacheVersion, StorageKey,
    TrackState,
};
use neo_vm::VmState;
use std::marker::PhantomData;
use std::rc::Rc;

/// Canonical transaction position and immutable execution inputs used for one
/// ordered validation decision.
#[derive(Clone, Debug, PartialEq)]
pub struct OptimisticApplicationContext {
    prefix: BlockPrefixIdentity,
    identity: SpeculativeExecutionIdentity,
}

impl OptimisticApplicationContext {
    /// Creates a context from independently established canonical inputs.
    #[must_use]
    pub(crate) const fn new(
        prefix: BlockPrefixIdentity,
        identity: SpeculativeExecutionIdentity,
    ) -> Self {
        Self { prefix, identity }
    }

    /// Captures immutable execution identity from a caller-owned current engine.
    ///
    /// The engine should be constructed from the transaction's canonical block
    /// context rather than borrowed from the speculative artifact itself.
    pub(crate) fn capture<P, D, B>(
        prefix: BlockPrefixIdentity,
        engine: &ApplicationEngine<P, D, B>,
    ) -> Result<Self, SpeculativeArtifactCaptureError>
    where
        P: NativeContractProvider + 'static,
        D: Diagnostic + 'static,
        B: CacheRead,
    {
        Ok(Self::new(prefix, capture_identity(engine)?))
    }

    /// Current canonical prefix identity.
    #[must_use]
    pub const fn prefix(&self) -> BlockPrefixIdentity {
        self.prefix
    }

    /// Current immutable execution identity.
    #[must_use]
    pub const fn identity(&self) -> &SpeculativeExecutionIdentity {
        &self.identity
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ResolvedContextDependency {
    observation_index: usize,
    access: HostContextAccess,
    value: Option<OptimisticContextValue>,
}

/// Single-use token for one ordered validation/application decision.
///
/// Construction is crate-private: the future scheduler must create this token
/// while it exclusively owns the canonical transaction-publication lane. The
/// pinned [`BlockPrefixIdentity`] is the lane's version token, and consuming the
/// guard prevents the validation context from being reused for another apply.
pub struct OptimisticApplicationGuard<'a, C: CacheRead> {
    context: OptimisticApplicationContext,
    canonical_cache: &'a DataCache<C>,
    cache_version: DataCacheVersion,
    resolved_contexts: Vec<ResolvedContextDependency>,
    _not_send_or_sync: PhantomData<Rc<()>>,
}

impl<'a, C: CacheRead> OptimisticApplicationGuard<'a, C> {
    pub(crate) fn capture<P, D, B>(
        prefix: BlockPrefixIdentity,
        engine: &ApplicationEngine<P, D, B>,
        canonical_cache: &'a DataCache<C>,
        artifact: &SpeculativeExecutionArtifact<P, D, B>,
        mut context_lookup: impl FnMut(usize, HostContextAccess) -> Option<OptimisticContextValue>,
    ) -> Result<Self, SpeculativeArtifactCaptureError>
    where
        P: NativeContractProvider + 'static,
        D: Diagnostic + 'static,
        B: CacheRead,
    {
        let context = OptimisticApplicationContext::capture(prefix, engine)?;
        let resolved_contexts = artifact
            .host_dependencies()
            .contexts()
            .iter()
            .map(|dependency| ResolvedContextDependency {
                observation_index: dependency.observation_index(),
                access: dependency.access(),
                value: context_lookup(dependency.observation_index(), dependency.access()),
            })
            .collect();
        // Capture this only after all caller-provided context resolution has
        // completed. Any later cache mutation invalidates the application
        // attempt before dependency validation or publication.
        let cache_version = canonical_cache.version();
        Ok(Self {
            context,
            canonical_cache,
            cache_version,
            resolved_contexts,
            _not_send_or_sync: PhantomData,
        })
    }

    fn context_value(
        &self,
        observation_index: usize,
        access: HostContextAccess,
    ) -> Option<OptimisticContextValue> {
        self.resolved_contexts
            .get(observation_index)
            .filter(|dependency| {
                dependency.observation_index == observation_index && dependency.access == access
            })
            .and_then(|dependency| dependency.value.clone())
    }
}

/// Exact prefix condition that prevented ordered artifact application.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OptimisticPrefixConflict {
    /// The artifact belongs to another block hash.
    BlockHash {
        /// Hash pinned by speculative execution.
        pinned: UInt256,
        /// Hash at the current canonical position.
        current: UInt256,
    },
    /// The artifact belongs to another block height.
    BlockIndex {
        /// Height pinned by speculative execution.
        pinned: u32,
        /// Height at the current canonical position.
        current: u32,
    },
    /// The caller attempted to validate against a prefix older than the pinned
    /// speculative snapshot.
    CurrentPrefixPrecedesPinned {
        /// Transactions represented by the pinned speculative snapshot.
        pinned_transactions: usize,
        /// Transactions represented by the current canonical prefix.
        current_transactions: usize,
    },
    /// The artifact is not the next transaction at the canonical position.
    TransactionPosition {
        /// Transaction position owned by the artifact.
        artifact_transaction: usize,
        /// Next transaction position in the current canonical prefix.
        current_transactions: usize,
    },
}

/// Fail-closed reason why a consumed artifact was not published.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OptimisticApplicationRejectionReason {
    /// Canonical block or transaction-prefix identity did not match.
    Prefix(OptimisticPrefixConflict),
    /// An immutable execution input changed.
    Identity(SpeculativeIdentityComponent),
    /// The foundation has no authoritative native-cache effect publisher.
    UnsupportedNativeCache {
        /// Native-cache locations whose pinned values would need validation.
        dependencies: usize,
        /// Ordered native-cache writes that would need publication.
        effects: usize,
    },
    /// Only terminal HALT and FAULT artifacts can be considered canonical.
    UnsupportedVmState(VmState),
    /// A detached change-set entry was not a publishable mutation.
    UnsupportedStorageState {
        /// Position in canonical storage-key order.
        effect_index: usize,
        /// Exact storage key.
        key: StorageKey,
        /// Unexpected cache tracking state.
        state: TrackState,
    },
    /// Every published write must retain the pinned value it was based on.
    MissingStorageDependency {
        /// Position in canonical storage-key order.
        effect_index: usize,
        /// Exact storage key.
        key: StorageKey,
    },
    /// A present or absent point read changed in the canonical prefix.
    PointReadConflict(super::PointReadConflict),
    /// The application guard was captured from another cache state.
    ForeignCanonicalCacheVersion,
    /// The canonical cache changed after the application guard was captured.
    StaleCanonicalCacheVersion {
        /// Revision pinned after canonical context resolution.
        expected: u64,
        /// Revision observed under the exclusive publication lock.
        actual: u64,
    },
    /// The complete storage-effect batch could not be published.
    StorageMerge(DataCacheError),
    /// The caller could not prove one observed context value.
    ContextUnproven {
        /// Observation order within the speculative execution.
        observation_index: usize,
        /// Exact host context dependency.
        access: HostContextAccess,
    },
    /// A host context value changed in the canonical prefix.
    HostDependencyConflict(HostDependencyValidation),
    /// A HALT artifact has storage effects but the destination is read-only.
    ReadOnlyCanonicalCache,
}

/// Successfully validated speculative result and its original NeoVM engine.
pub struct AppliedSpeculativeExecution<P, D, B = neo_storage::EmptyCacheBacking>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: CacheRead,
{
    engine: ApplicationEngine<P, D, B>,
    validated_point_reads: usize,
    validated_contexts: usize,
    applied_storage_effects: usize,
    discarded_fault_storage_effects: usize,
}

impl<P, D, B> AppliedSpeculativeExecution<P, D, B>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: CacheRead,
{
    /// Original engine, including its NeoVM stacks, gas, fault, diagnostics,
    /// notifications, logs, calls, invocations, and witness-visible state.
    #[must_use]
    pub const fn engine(&self) -> &ApplicationEngine<P, D, B> {
        &self.engine
    }

    /// Number of exact point dependencies validated.
    #[must_use]
    pub const fn validated_point_reads(&self) -> usize {
        self.validated_point_reads
    }

    /// Number of ordered host-context observations validated.
    #[must_use]
    pub const fn validated_contexts(&self) -> usize {
        self.validated_contexts
    }

    /// Number of HALT storage effects merged into the canonical cache.
    #[must_use]
    pub const fn applied_storage_effects(&self) -> usize {
        self.applied_storage_effects
    }

    /// Number of detached storage effects discarded because the engine faulted.
    #[must_use]
    pub const fn discarded_fault_storage_effects(&self) -> usize {
        self.discarded_fault_storage_effects
    }

    /// Consumes the application result and returns the original engine.
    #[must_use]
    pub fn into_engine(self) -> ApplicationEngine<P, D, B> {
        self.engine
    }
}

/// Rejected artifact after its detached effects have been made unreachable.
///
/// Only the original engine is returned. The artifact itself is deliberately
/// not returned, so a caller cannot accidentally validate or apply it twice.
pub struct RejectedSpeculativeExecution<P, D, B = neo_storage::EmptyCacheBacking>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: CacheRead,
{
    reason: OptimisticApplicationRejectionReason,
    engine: ApplicationEngine<P, D, B>,
}

impl<P, D, B> RejectedSpeculativeExecution<P, D, B>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: CacheRead,
{
    /// Deterministic reason that requires sequential fallback.
    #[must_use]
    pub const fn reason(&self) -> &OptimisticApplicationRejectionReason {
        &self.reason
    }

    /// Original speculative engine retained for diagnostics or comparison.
    #[must_use]
    pub const fn engine(&self) -> &ApplicationEngine<P, D, B> {
        &self.engine
    }

    /// Consumes the rejection and returns the original engine only.
    #[must_use]
    pub fn into_engine(self) -> ApplicationEngine<P, D, B> {
        self.engine
    }
}

/// Result of consuming exactly one speculative artifact at its canonical
/// transaction position.
pub type OptimisticApplicationResult<P, D, B> =
    Result<AppliedSpeculativeExecution<P, D, B>, RejectedSpeculativeExecution<P, D, B>>;

/// Validates and applies one speculative artifact exactly once.
///
/// All prefix, identity, support, point-read, and context checks complete
/// before the destination cache is mutated. Point dependency validation and
/// HALT storage publication occur under one exclusive cache lock through
/// [`DataCache::try_validate_and_merge_tracked_items`], preserving the
/// destination's current-prefix `TrackState` transitions without a
/// check-then-merge race. FAULT effects are discarded but their dependencies
/// are validated under the same versioned lock. The returned success or
/// rejection owns only the original engine, making the consumed artifact
/// unavailable for a second application attempt.
///
/// Both the artifact and crate-sealed guard are consumed by this call. There is
/// no API that can validate or apply either value a second time.
pub fn validate_and_apply_artifact<P, D, B, C>(
    artifact: SpeculativeExecutionArtifact<P, D, B>,
    guard: OptimisticApplicationGuard<'_, C>,
) -> OptimisticApplicationResult<P, D, B>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: CacheRead,
    C: CacheRead,
{
    if let Some(conflict) = first_prefix_conflict(&artifact, guard.context.prefix()) {
        return reject(
            artifact,
            OptimisticApplicationRejectionReason::Prefix(conflict),
        );
    }

    if let Some(component) = artifact.identity().first_mismatch(guard.context.identity()) {
        return reject(
            artifact,
            OptimisticApplicationRejectionReason::Identity(component),
        );
    }

    let state = artifact.vm_state();
    if state != VmState::HALT && state != VmState::FAULT {
        return reject(
            artifact,
            OptimisticApplicationRejectionReason::UnsupportedVmState(state),
        );
    }

    let native_dependencies = artifact.host_dependencies().native_cache().len();
    let native_effects = artifact.host_dependencies().native_effects().len();
    if native_dependencies != 0 || native_effects != 0 {
        return reject(
            artifact,
            OptimisticApplicationRejectionReason::UnsupportedNativeCache {
                dependencies: native_dependencies,
                effects: native_effects,
            },
        );
    }

    let storage_rejection =
        artifact
            .storage_effects()
            .iter()
            .enumerate()
            .find_map(|(effect_index, effect)| {
                if !matches!(
                    effect.trackable().state,
                    TrackState::Added | TrackState::Changed | TrackState::Deleted
                ) {
                    return Some(
                        OptimisticApplicationRejectionReason::UnsupportedStorageState {
                            effect_index,
                            key: effect.key().clone(),
                            state: effect.trackable().state,
                        },
                    );
                }
                if artifact
                    .point_dependencies()
                    .point_reads()
                    .binary_search_by(|dependency| dependency.key().cmp(effect.key()))
                    .is_err()
                {
                    return Some(
                        OptimisticApplicationRejectionReason::MissingStorageDependency {
                            effect_index,
                            key: effect.key().clone(),
                        },
                    );
                }
                None
            });
    if let Some(reason) = storage_rejection {
        return reject(artifact, reason);
    }

    if state == VmState::HALT
        && !artifact.storage_effects().is_empty()
        && guard.canonical_cache.is_read_only()
    {
        return reject(
            artifact,
            OptimisticApplicationRejectionReason::ReadOnlyCanonicalCache,
        );
    }

    #[derive(Clone, Copy)]
    struct ContextUnproven {
        observation_index: usize,
        access: HostContextAccess,
    }

    let host_validation =
        artifact
            .host_dependencies()
            .try_revalidate_contexts(|observation_index, access| {
                guard
                    .context_value(observation_index, access)
                    .ok_or(ContextUnproven {
                        observation_index,
                        access,
                    })
            });
    let validated_contexts = match host_validation {
        Err(unproven) => {
            return reject(
                artifact,
                OptimisticApplicationRejectionReason::ContextUnproven {
                    observation_index: unproven.observation_index,
                    access: unproven.access,
                },
            );
        }
        Ok(HostDependencyValidation::Valid { contexts, .. }) => contexts,
        Ok(conflict) => {
            return reject(
                artifact,
                OptimisticApplicationRejectionReason::HostDependencyConflict(conflict),
            );
        }
    };

    let storage_effect_count = artifact.storage_effects().len();
    let (engine, point_dependencies, storage_effects) = artifact.into_application_parts();
    let effects_to_publish = if state == VmState::HALT {
        storage_effects.as_slice()
    } else {
        &[]
    };
    let atomic_result = guard.canonical_cache.try_validate_and_merge_tracked_items(
        &guard.cache_version,
        effects_to_publish,
        |view| match point_dependencies.revalidate_point_reads(|key| view.get(key)) {
            PointReadValidation::Valid {
                checked_point_reads,
            } => Ok(checked_point_reads),
            PointReadValidation::Conflict(conflict) => Err(conflict),
        },
    );
    let validated_point_reads = match atomic_result {
        Ok(validated_point_reads) => validated_point_reads,
        Err(DataCacheAtomicMergeError::ForeignVersion) => {
            return reject_engine(
                engine,
                OptimisticApplicationRejectionReason::ForeignCanonicalCacheVersion,
            );
        }
        Err(DataCacheAtomicMergeError::StaleVersion { expected, actual }) => {
            return reject_engine(
                engine,
                OptimisticApplicationRejectionReason::StaleCanonicalCacheVersion {
                    expected,
                    actual,
                },
            );
        }
        Err(DataCacheAtomicMergeError::Validation(conflict)) => {
            return reject_engine(
                engine,
                OptimisticApplicationRejectionReason::PointReadConflict(conflict),
            );
        }
        Err(DataCacheAtomicMergeError::Merge(error)) => {
            return reject_engine(
                engine,
                OptimisticApplicationRejectionReason::StorageMerge(error),
            );
        }
    };

    Ok(AppliedSpeculativeExecution {
        engine,
        validated_point_reads,
        validated_contexts,
        applied_storage_effects: if state == VmState::HALT {
            storage_effect_count
        } else {
            0
        },
        discarded_fault_storage_effects: if state == VmState::FAULT {
            storage_effect_count
        } else {
            0
        },
    })
}

fn first_prefix_conflict<P, D, B>(
    artifact: &SpeculativeExecutionArtifact<P, D, B>,
    current: BlockPrefixIdentity,
) -> Option<OptimisticPrefixConflict>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: CacheRead,
{
    let pinned = artifact.prefix();
    if pinned.block_hash() != current.block_hash() {
        return Some(OptimisticPrefixConflict::BlockHash {
            pinned: pinned.block_hash(),
            current: current.block_hash(),
        });
    }
    if pinned.block_index() != current.block_index() {
        return Some(OptimisticPrefixConflict::BlockIndex {
            pinned: pinned.block_index(),
            current: current.block_index(),
        });
    }
    if current.applied_transactions() < pinned.applied_transactions() {
        return Some(OptimisticPrefixConflict::CurrentPrefixPrecedesPinned {
            pinned_transactions: pinned.applied_transactions(),
            current_transactions: current.applied_transactions(),
        });
    }
    if artifact.transaction_index() != current.applied_transactions() {
        return Some(OptimisticPrefixConflict::TransactionPosition {
            artifact_transaction: artifact.transaction_index(),
            current_transactions: current.applied_transactions(),
        });
    }
    None
}

fn reject<P, D, B>(
    artifact: SpeculativeExecutionArtifact<P, D, B>,
    reason: OptimisticApplicationRejectionReason,
) -> OptimisticApplicationResult<P, D, B>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: CacheRead,
{
    Err(RejectedSpeculativeExecution {
        reason,
        engine: artifact.into_engine(),
    })
}

fn reject_engine<P, D, B>(
    engine: ApplicationEngine<P, D, B>,
    reason: OptimisticApplicationRejectionReason,
) -> OptimisticApplicationResult<P, D, B>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: CacheRead,
{
    Err(RejectedSpeculativeExecution { reason, engine })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application_engine::TEST_MODE_GAS;
    use crate::execution_artifact::{ContextObservationValue, ExecutionArtifactLimits};
    use crate::host_access_audit::{
        NativeCacheAccess, NativeCacheAccessKind, ResolvedNativeCacheScope,
    };
    use crate::native_contract_provider::NoNativeContractProvider;
    use crate::{NoDiagnostic, TriggerType};
    use neo_config::ProtocolSettings;
    use neo_primitives::{CallFlags, UInt160};
    use neo_storage::{EmptyCacheBacking, StorageItem};
    use neo_vm::{NativeCacheDomain, OpCode};
    use std::cell::RefCell;
    use std::sync::Arc;

    type TestArtifact =
        SpeculativeExecutionArtifact<NoNativeContractProvider, NoDiagnostic, EmptyCacheBacking>;

    const STORAGE_EFFECT_SYSCALL: &str = "Test.OptimisticStorageEffects";

    #[derive(Clone)]
    enum TestStorageMutation {
        Set(StorageKey, StorageItem),
        Delete(StorageKey),
        BlindAdd(StorageKey, StorageItem),
    }

    thread_local! {
        static TEST_STORAGE_MUTATIONS: RefCell<Vec<TestStorageMutation>> = const {
            RefCell::new(Vec::new())
        };
    }

    fn apply_test_storage_effects(
        engine: &mut ApplicationEngine,
        _vm: &mut crate::ApplicationExecutionEngine,
    ) -> neo_vm::VmResult<()> {
        let mutations =
            TEST_STORAGE_MUTATIONS.with(|mutations| std::mem::take(&mut *mutations.borrow_mut()));
        for mutation in mutations {
            match mutation {
                TestStorageMutation::Set(key, value) => engine
                    .set_storage(key, value)
                    .map_err(|error| neo_vm::VmError::invalid_operation_msg(error.to_string()))?,
                TestStorageMutation::Delete(key) => engine
                    .delete_storage(&key)
                    .map_err(|error| neo_vm::VmError::invalid_operation_msg(error.to_string()))?,
                TestStorageMutation::BlindAdd(key, value) => {
                    engine.snapshot_cache().add(key, value);
                }
            }
        }
        Ok(())
    }

    fn storage_effect_script(tail: &[u8]) -> Vec<u8> {
        let mut script = vec![OpCode::SYSCALL.byte()];
        script.extend_from_slice(&neo_vm::interop_hash(STORAGE_EFFECT_SYSCALL).to_le_bytes());
        script.extend_from_slice(tail);
        script
    }

    fn register_test_storage_effects(engine: &mut ApplicationEngine) {
        engine
            .register_host_service(
                STORAGE_EFFECT_SYSCALL,
                0,
                CallFlags::NONE,
                apply_test_storage_effects,
            )
            .expect("test storage-effect syscall");
    }

    fn key(suffix: &[u8]) -> StorageKey {
        StorageKey::new(7, suffix.to_vec())
    }

    fn item(value: &[u8]) -> StorageItem {
        StorageItem::from_bytes(value.to_vec())
    }

    fn prefix(applied_transactions: usize) -> BlockPrefixIdentity {
        BlockPrefixIdentity::new(UInt256::default(), 42, applied_transactions)
    }

    fn make_engine(
        cache: Arc<DataCache<EmptyCacheBacking>>,
        trigger: TriggerType,
    ) -> ApplicationEngine {
        ApplicationEngine::<NoNativeContractProvider>::new_with_shared_block_and_native_contract_provider(
            trigger,
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

    struct ArtifactBuilder {
        engine: ApplicationEngine,
        overlay: super::super::IsolatedTransactionOverlay<EmptyCacheBacking>,
        point_capture: super::super::TransactionDependencyCapture,
        observations: super::super::OptimisticObservationBinding,
        mutations: Vec<TestStorageMutation>,
    }

    impl ArtifactBuilder {
        fn load(&mut self, script: Vec<u8>) {
            self.engine
                .load_script(script, CallFlags::ALL, None)
                .expect("load script");
        }

        fn set_storage(&mut self, key: StorageKey, value: StorageItem) {
            self.mutations.push(TestStorageMutation::Set(key, value));
        }

        fn delete_storage(&mut self, key: StorageKey) {
            self.mutations.push(TestStorageMutation::Delete(key));
        }

        fn blind_add(&mut self, key: StorageKey, value: StorageItem) {
            self.mutations
                .push(TestStorageMutation::BlindAdd(key, value));
        }

        fn capture_with_tail(mut self, tail: &[u8]) -> TestArtifact {
            self.load(storage_effect_script(tail));
            self.capture()
        }

        fn capture(self) -> TestArtifact {
            let previous =
                TEST_STORAGE_MUTATIONS.with(|mutations| mutations.replace(self.mutations));
            assert!(
                previous.is_empty(),
                "test mutation lane was already occupied"
            );
            SpeculativeExecutionArtifact::execute_and_capture(
                self.engine,
                self.overlay,
                self.point_capture,
                &self.observations,
            )
            .expect("speculative artifact")
        }
    }

    fn artifact_builder(
        canonical: &DataCache<EmptyCacheBacking>,
        pinned_transactions: usize,
        transaction_index: usize,
    ) -> ArtifactBuilder {
        let pinned =
            super::super::PinnedBlockPrefix::capture(prefix(pinned_transactions), canonical);
        let (overlay, point_capture) = pinned
            .transaction_overlay_with_dependency_capture(
                transaction_index,
                super::super::DependencyCaptureLimits::default(),
            )
            .expect("transaction overlay");
        let mut engine = make_engine(overlay.snapshot_cache(), TriggerType::Application);
        register_test_storage_effects(&mut engine);
        let observations =
            super::super::OptimisticObservationBinding::new(ExecutionArtifactLimits::default());
        observations.bind(&mut engine).expect("observation binding");
        ArtifactBuilder {
            engine,
            overlay,
            point_capture,
            observations,
            mutations: Vec::new(),
        }
    }

    fn finish_engine(builder: ArtifactBuilder, opcode: OpCode) -> TestArtifact {
        builder.capture_with_tail(&[opcode.byte()])
    }

    fn current_guard<'a>(
        artifact: &TestArtifact,
        canonical: &'a DataCache<EmptyCacheBacking>,
        applied_transactions: usize,
        trigger: TriggerType,
        script: &[u8],
        context_lookup: impl FnMut(usize, HostContextAccess) -> Option<OptimisticContextValue>,
    ) -> OptimisticApplicationGuard<'a, EmptyCacheBacking> {
        let mut engine = make_engine(Arc::new(canonical.clone()), trigger);
        register_test_storage_effects(&mut engine);
        engine
            .load_script_bytes(script, CallFlags::ALL, None)
            .expect("load canonical script");
        OptimisticApplicationGuard::capture(
            prefix(applied_transactions),
            &engine,
            canonical,
            artifact,
            context_lookup,
        )
        .expect("current application guard")
    }

    fn tracked_state(
        cache: &DataCache<EmptyCacheBacking>,
        wanted: &StorageKey,
    ) -> Option<TrackState> {
        let mut found = None;
        cache.visit_tracked_items(|key, trackable| {
            if key == wanted {
                found = Some(trackable.state);
            }
        });
        found
    }

    #[test]
    fn halt_applies_once_with_current_prefix_track_state_semantics() {
        let canonical = DataCache::new(false);
        let persisted_changed = key(b"persisted-changed");
        let persisted_deleted = key(b"persisted-deleted");
        let prefix_deleted_readded = key(b"prefix-deleted-readded");
        let prefix_added_changed = key(b"prefix-added-changed");
        let prefix_added_deleted = key(b"prefix-added-deleted");
        let transaction_added = StorageKey::new(-5, b"native-contract-storage".to_vec());

        canonical.add(persisted_changed.clone(), item(b"old"));
        canonical.add(persisted_deleted.clone(), item(b"delete-me"));
        canonical.add(
            prefix_deleted_readded.clone(),
            item(b"delete-before-prefix"),
        );
        canonical.commit();
        canonical.delete(&prefix_deleted_readded);
        canonical.add(prefix_added_changed.clone(), item(b"prefix"));
        canonical.add(prefix_added_deleted.clone(), item(b"prefix"));

        let mut builder = artifact_builder(&canonical, 0, 0);
        let speculative_cache = builder.engine.original_snapshot_cache_handle();
        builder.set_storage(persisted_changed.clone(), item(b"changed"));
        builder.set_storage(prefix_added_changed.clone(), item(b"changed"));
        builder.set_storage(transaction_added.clone(), item(b"new"));
        builder.set_storage(prefix_deleted_readded.clone(), item(b"readded"));
        builder.delete_storage(persisted_deleted.clone());
        builder.delete_storage(prefix_added_deleted.clone());
        let artifact = builder.capture_with_tail(&[OpCode::PUSH7.byte(), OpCode::RET.byte()]);
        let canonical_script = storage_effect_script(&[OpCode::PUSH7.byte(), OpCode::RET.byte()]);
        let current = current_guard(
            &artifact,
            &canonical,
            0,
            TriggerType::Application,
            &canonical_script,
            |_, _| None,
        );

        let applied = match validate_and_apply_artifact(artifact, current) {
            Ok(applied) => applied,
            Err(rejected) => panic!("unexpected rejection: {:?}", rejected.reason()),
        };

        assert_eq!(applied.applied_storage_effects(), 6);
        assert_eq!(applied.discarded_fault_storage_effects(), 0);
        assert_eq!(applied.validated_point_reads(), 6);
        assert_eq!(applied.engine().state(), VmState::HALT);
        assert!(applied.engine().gas_consumed_pico() > 0);
        assert!(applied.engine().result_stack().peek(0).is_ok());
        assert!(Arc::ptr_eq(
            &speculative_cache,
            &applied.engine().original_snapshot_cache_handle()
        ));
        assert_eq!(canonical.get(&persisted_changed), Some(item(b"changed")));
        assert_eq!(canonical.get(&prefix_added_changed), Some(item(b"changed")));
        assert_eq!(canonical.get(&transaction_added), Some(item(b"new")));
        assert_eq!(
            canonical.get(&prefix_deleted_readded),
            Some(item(b"readded"))
        );
        assert_eq!(canonical.get(&persisted_deleted), None);
        assert_eq!(canonical.get(&prefix_added_deleted), None);
        assert_eq!(
            tracked_state(&canonical, &persisted_changed),
            Some(TrackState::Changed)
        );
        assert_eq!(
            tracked_state(&canonical, &prefix_added_changed),
            Some(TrackState::Added)
        );
        assert_eq!(
            tracked_state(&canonical, &transaction_added),
            Some(TrackState::Added)
        );
        assert_eq!(
            tracked_state(&canonical, &prefix_deleted_readded),
            Some(TrackState::Changed)
        );
        assert_eq!(
            tracked_state(&canonical, &persisted_deleted),
            Some(TrackState::Deleted)
        );
        assert_eq!(tracked_state(&canonical, &prefix_added_deleted), None);

        // `applied` contains only the original engine. The artifact was moved
        // into the call, so there is no value or API with which to apply it a
        // second time (also enforced by the compile-fail example above).
        let engine = applied.into_engine();
        assert_eq!(engine.state(), VmState::HALT);
    }

    #[test]
    fn write_write_conflict_rejects_without_partial_publication() {
        let canonical = DataCache::new(false);
        let storage_key = key(b"shared");
        canonical.add(storage_key.clone(), item(b"pinned"));
        canonical.commit();

        let mut builder = artifact_builder(&canonical, 0, 1);
        builder.set_storage(storage_key.clone(), item(b"speculative"));
        let artifact = finish_engine(builder, OpCode::RET);

        canonical.update(storage_key.clone(), item(b"earlier-transaction"));
        let canonical_script = storage_effect_script(&[OpCode::RET.byte()]);
        let current = current_guard(
            &artifact,
            &canonical,
            1,
            TriggerType::Application,
            &canonical_script,
            |_, _| None,
        );
        let rejected = match validate_and_apply_artifact(artifact, current) {
            Ok(_) => panic!("conflicting artifact applied"),
            Err(rejected) => rejected,
        };

        assert!(matches!(
            rejected.reason(),
            OptimisticApplicationRejectionReason::PointReadConflict(conflict)
                if conflict.key() == &storage_key
                    && conflict.kind() == super::super::PointReadConflictKind::PresentValueChanged
        ));
        assert_eq!(rejected.engine().state(), VmState::HALT);
        assert_eq!(
            canonical.get(&storage_key),
            Some(item(b"earlier-transaction"))
        );
    }

    #[test]
    fn stale_application_guard_rejects_without_publication() {
        let canonical = DataCache::new(false);
        let storage_key = key(b"guarded-write");
        let unrelated_key = key(b"later-cache-mutation");
        canonical.add(storage_key.clone(), item(b"pinned"));
        canonical.commit();

        let mut builder = artifact_builder(&canonical, 0, 0);
        builder.set_storage(storage_key.clone(), item(b"speculative"));
        let artifact = finish_engine(builder, OpCode::RET);
        let canonical_script = storage_effect_script(&[OpCode::RET.byte()]);
        let current = current_guard(
            &artifact,
            &canonical,
            0,
            TriggerType::Application,
            &canonical_script,
            |_, _| None,
        );

        canonical.add(unrelated_key.clone(), item(b"canonical"));
        let rejected = validate_and_apply_artifact(artifact, current)
            .err()
            .expect("stale cache version rejection");

        assert!(matches!(
            rejected.reason(),
            OptimisticApplicationRejectionReason::StaleCanonicalCacheVersion {
                expected,
                actual,
            } if actual > expected
        ));
        assert_eq!(canonical.get(&storage_key), Some(item(b"pinned")));
        assert_eq!(canonical.get(&unrelated_key), Some(item(b"canonical")));
    }

    #[test]
    fn fault_retains_engine_and_discards_all_storage() {
        let canonical = DataCache::new(false);
        let storage_key = key(b"fault-write");
        let mut builder = artifact_builder(&canonical, 0, 0);
        builder.set_storage(storage_key.clone(), item(b"must-discard"));
        let artifact = finish_engine(builder, OpCode::ABORT);
        let canonical_script = storage_effect_script(&[OpCode::ABORT.byte()]);
        let current = current_guard(
            &artifact,
            &canonical,
            0,
            TriggerType::Application,
            &canonical_script,
            |_, _| None,
        );

        let applied = match validate_and_apply_artifact(artifact, current) {
            Ok(applied) => applied,
            Err(rejected) => panic!("unexpected rejection: {:?}", rejected.reason()),
        };

        assert_eq!(applied.engine().state(), VmState::FAULT);
        assert!(applied.engine().fault_exception().is_some());
        assert_eq!(applied.applied_storage_effects(), 0);
        assert_eq!(applied.discarded_fault_storage_effects(), 0);
        assert_eq!(canonical.get(&storage_key), None);
        assert_eq!(canonical.pending_change_count(), 0);
    }

    #[test]
    fn wrong_position_and_identity_fail_before_storage_application() {
        let canonical = DataCache::new(false);
        let storage_key = key(b"position");
        let mut builder = artifact_builder(&canonical, 0, 1);
        builder.set_storage(storage_key.clone(), item(b"speculative"));
        let artifact = finish_engine(builder, OpCode::RET);
        let canonical_script = storage_effect_script(&[OpCode::RET.byte()]);
        let wrong_position = current_guard(
            &artifact,
            &canonical,
            0,
            TriggerType::Application,
            &canonical_script,
            |_, _| None,
        );

        let rejected = validate_and_apply_artifact(artifact, wrong_position)
            .err()
            .expect("position rejection");
        assert!(matches!(
            rejected.reason(),
            OptimisticApplicationRejectionReason::Prefix(
                OptimisticPrefixConflict::TransactionPosition {
                    artifact_transaction: 1,
                    current_transactions: 0,
                }
            )
        ));
        assert_eq!(canonical.get(&storage_key), None);

        let mut builder = artifact_builder(&canonical, 0, 0);
        builder.set_storage(storage_key.clone(), item(b"speculative"));
        let artifact = finish_engine(builder, OpCode::RET);
        let canonical_script = storage_effect_script(&[OpCode::RET.byte()]);
        let wrong_identity = current_guard(
            &artifact,
            &canonical,
            0,
            TriggerType::Verification,
            &canonical_script,
            |_, _| None,
        );
        let rejected = validate_and_apply_artifact(artifact, wrong_identity)
            .err()
            .expect("identity rejection");
        assert_eq!(
            rejected.reason(),
            &OptimisticApplicationRejectionReason::Identity(SpeculativeIdentityComponent::Trigger)
        );
        assert_eq!(canonical.get(&storage_key), None);
    }

    #[test]
    fn context_dependency_must_be_proven_and_equal() {
        let canonical = DataCache::new(false);
        let builder = artifact_builder(&canonical, 0, 0);
        builder.engine.observe_context(
            HostContextAccess::Network,
            ContextObservationValue::U32(860_833_102),
        );
        let artifact = finish_engine(builder, OpCode::RET);
        let canonical_script = storage_effect_script(&[OpCode::RET.byte()]);

        let current = current_guard(
            &artifact,
            &canonical,
            0,
            TriggerType::Application,
            &canonical_script,
            |_, _| Some(OptimisticContextValue::U32(1)),
        );
        let rejected = validate_and_apply_artifact(artifact, current)
            .err()
            .expect("context conflict");
        assert!(matches!(
            rejected.reason(),
            OptimisticApplicationRejectionReason::HostDependencyConflict(
                HostDependencyValidation::ContextConflict {
                    observation_index: 0,
                    access: HostContextAccess::Network,
                }
            )
        ));

        let builder = artifact_builder(&canonical, 0, 0);
        builder.engine.observe_context(
            HostContextAccess::Network,
            ContextObservationValue::U32(860_833_102),
        );
        let artifact = finish_engine(builder, OpCode::RET);
        let canonical_script = storage_effect_script(&[OpCode::RET.byte()]);
        let current = current_guard(
            &artifact,
            &canonical,
            0,
            TriggerType::Application,
            &canonical_script,
            |_, _| None,
        );
        let rejected = validate_and_apply_artifact(artifact, current)
            .err()
            .expect("unproven context");
        assert_eq!(
            rejected.reason(),
            &OptimisticApplicationRejectionReason::ContextUnproven {
                observation_index: 0,
                access: HostContextAccess::Network,
            }
        );
    }

    #[test]
    fn native_cache_observation_fails_closed_without_publisher() {
        let canonical = DataCache::new(false);
        let builder = artifact_builder(&canonical, 0, 0);
        builder
            .observations
            .record_native_cache(
                NativeCacheAccess::new(
                    NativeCacheDomain {
                        contract_hash: UInt160::zero(),
                        contract_id: -5,
                        native_version: 0,
                        partition: 1,
                    },
                    ResolvedNativeCacheScope::WholeDomain,
                    NativeCacheAccessKind::Read,
                ),
                Some(b"native".to_vec()),
                Some(b"native".to_vec()),
            )
            .expect("native observation");
        let artifact = finish_engine(builder, OpCode::RET);
        let canonical_script = storage_effect_script(&[OpCode::RET.byte()]);
        let current = current_guard(
            &artifact,
            &canonical,
            0,
            TriggerType::Application,
            &canonical_script,
            |_, _| None,
        );

        let rejected = validate_and_apply_artifact(artifact, current)
            .err()
            .expect("native-cache rejection");
        assert_eq!(
            rejected.reason(),
            &OptimisticApplicationRejectionReason::UnsupportedNativeCache {
                dependencies: 1,
                effects: 0,
            }
        );
    }

    #[test]
    fn write_without_pinned_dependency_fails_closed() {
        let canonical = DataCache::new(false);
        let storage_key = StorageKey::new(-5, b"native-storage-is-still-storage".to_vec());
        let mut builder = artifact_builder(&canonical, 0, 0);
        builder.blind_add(storage_key.clone(), item(b"unobserved"));
        let artifact = finish_engine(builder, OpCode::RET);
        let canonical_script = storage_effect_script(&[OpCode::RET.byte()]);
        let current = current_guard(
            &artifact,
            &canonical,
            0,
            TriggerType::Application,
            &canonical_script,
            |_, _| None,
        );

        let rejected = validate_and_apply_artifact(artifact, current)
            .err()
            .expect("missing dependency rejection");
        assert!(matches!(
            rejected.reason(),
            OptimisticApplicationRejectionReason::MissingStorageDependency { key, .. }
                if key == &storage_key
        ));
        assert_eq!(canonical.get(&storage_key), None);
    }

    #[test]
    fn post_capture_overlay_mutation_cannot_change_sealed_effects() {
        let canonical = DataCache::new(false);
        let storage_key = key(b"sealed-effect");
        canonical.add(storage_key.clone(), item(b"prefix"));
        canonical.commit();

        let mut builder = artifact_builder(&canonical, 0, 0);
        let retained_overlay_handle = builder.overlay.snapshot_cache();
        let retained_binding = builder.observations.clone();
        builder.set_storage(storage_key.clone(), item(b"captured"));
        let artifact = finish_engine(builder, OpCode::RET);

        retained_overlay_handle.update(storage_key.clone(), item(b"post-capture"));
        assert_eq!(
            retained_binding.record_native_cache(
                NativeCacheAccess::new(
                    NativeCacheDomain {
                        contract_hash: UInt160::zero(),
                        contract_id: -5,
                        native_version: 0,
                        partition: 1,
                    },
                    ResolvedNativeCacheScope::WholeDomain,
                    NativeCacheAccessKind::Read,
                ),
                None,
                None,
            ),
            Err(SpeculativeArtifactCaptureError::BindingSealed)
        );

        let canonical_script = storage_effect_script(&[OpCode::RET.byte()]);
        let current = current_guard(
            &artifact,
            &canonical,
            0,
            TriggerType::Application,
            &canonical_script,
            |_, _| None,
        );
        if let Err(rejected) = validate_and_apply_artifact(artifact, current) {
            panic!("sealed artifact rejected: {:?}", rejected.reason());
        }
        assert_eq!(canonical.get(&storage_key), Some(item(b"captured")));
    }

    #[test]
    fn exact_entry_script_bytes_are_part_of_application_identity() {
        let canonical = DataCache::new(false);
        let builder = artifact_builder(&canonical, 0, 0);
        let artifact = finish_engine(builder, OpCode::RET);
        let different_but_halting_script = [OpCode::NOP.byte(), OpCode::RET.byte()];
        let current = current_guard(
            &artifact,
            &canonical,
            0,
            TriggerType::Application,
            &different_but_halting_script,
            |_, _| None,
        );

        let rejected = validate_and_apply_artifact(artifact, current)
            .err()
            .expect("script identity rejection");
        assert_eq!(
            rejected.reason(),
            &OptimisticApplicationRejectionReason::Identity(
                SpeculativeIdentityComponent::EntryScript
            )
        );
    }
}
