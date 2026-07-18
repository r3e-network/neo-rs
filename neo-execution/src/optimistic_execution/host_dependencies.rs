use crate::execution_artifact::{
    CanonicalStackDocument, ContextObservationSnapshotValue, ExecutionObservationJournal,
};
use crate::host_access_audit::{
    HostContextAccess, NativeCacheAccess, NativeCacheAccessKind, ResolvedNativeCacheScope,
};
use neo_vm::NativeCacheDomain;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Exact context value retained for optimistic dependency validation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OptimisticContextValue {
    /// Boolean context result.
    Boolean(bool),
    /// Unsigned byte context result.
    U8(u8),
    /// Unsigned 32-bit context result.
    U32(u32),
    /// Unsigned 64-bit context result.
    U64(u64),
    /// Signed 64-bit context result.
    I64(i64),
    /// Trigger flags.
    Trigger(neo_primitives::TriggerType),
    /// Effective call flags encoded as their exact byte.
    CallFlags(u8),
    /// Optional executing/calling/entry script hash.
    Hash160(Option<neo_primitives::UInt160>),
    /// Optional script-container hash.
    Hash256(Option<neo_primitives::UInt256>),
    /// Canonical stack roots used by context queries such as notifications.
    StackItems(Arc<CanonicalStackDocument>),
}

impl OptimisticContextValue {
    /// Converts a bounded execution-journal context value without touching VM
    /// stack objects.
    fn from_snapshot(value: &ContextObservationSnapshotValue) -> Self {
        match value {
            ContextObservationSnapshotValue::Boolean(value) => Self::Boolean(*value),
            ContextObservationSnapshotValue::U8(value) => Self::U8(*value),
            ContextObservationSnapshotValue::U32(value) => Self::U32(*value),
            ContextObservationSnapshotValue::U64(value) => Self::U64(*value),
            ContextObservationSnapshotValue::I64(value) => Self::I64(*value),
            ContextObservationSnapshotValue::Trigger(value) => Self::Trigger(*value),
            ContextObservationSnapshotValue::CallFlags(value) => Self::CallFlags(*value),
            ContextObservationSnapshotValue::Hash160(value) => Self::Hash160(*value),
            ContextObservationSnapshotValue::Hash256(value) => Self::Hash256(*value),
            ContextObservationSnapshotValue::StackItems(value) => Self::StackItems(value.clone()),
        }
    }
}

/// One exact context observation in execution order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptimisticContextDependency {
    observation_index: usize,
    access: HostContextAccess,
    value: OptimisticContextValue,
}

impl OptimisticContextDependency {
    /// Observation order within the transaction artifact.
    #[must_use]
    pub const fn observation_index(&self) -> usize {
        self.observation_index
    }

    /// Exact host context identity.
    #[must_use]
    pub const fn access(&self) -> HostContextAccess {
        self.access
    }

    /// Exact observed value.
    #[must_use]
    pub const fn value(&self) -> &OptimisticContextValue {
        &self.value
    }
}

/// Stable identity for one native-cache entry or whole domain.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeCacheLocation {
    domain: NativeCacheDomain,
    scope: ResolvedNativeCacheScope,
}

impl NativeCacheLocation {
    fn from_access(access: &NativeCacheAccess) -> Self {
        Self {
            domain: access.domain(),
            scope: access.scope().clone(),
        }
    }

    /// Versioned native-cache domain.
    #[must_use]
    pub const fn domain(&self) -> NativeCacheDomain {
        self.domain
    }

    /// Exact entry or whole-domain scope.
    #[must_use]
    pub fn scope(&self) -> &ResolvedNativeCacheScope {
        &self.scope
    }
}

/// First pinned value required by one native-cache location.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeCacheDependency {
    location: NativeCacheLocation,
    expected: Option<Vec<u8>>,
}

impl NativeCacheDependency {
    /// Stable native-cache location.
    #[must_use]
    pub const fn location(&self) -> &NativeCacheLocation {
        &self.location
    }

    /// First value observed before local writes, including absence.
    #[must_use]
    pub fn expected(&self) -> Option<&[u8]> {
        self.expected.as_deref()
    }
}

/// One native-cache write effect retained in execution order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeCacheEffect {
    observation_index: usize,
    location: NativeCacheLocation,
    value: Option<Vec<u8>>,
}

impl NativeCacheEffect {
    /// Observation order within the transaction artifact.
    #[must_use]
    pub const fn observation_index(&self) -> usize {
        self.observation_index
    }

    /// Stable native-cache location.
    #[must_use]
    pub const fn location(&self) -> &NativeCacheLocation {
        &self.location
    }

    /// Value written by the effect, or `None` for deletion.
    #[must_use]
    pub fn value(&self) -> Option<&[u8]> {
        self.value.as_deref()
    }
}

/// Fail-closed reason while turning bounded live observations into optimistic
/// dependencies.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum HostDependencyCaptureError {
    /// A native-cache read changed its value during the same execution.
    #[error("native-cache read observation {observation_index} changed its value")]
    NativeReadChanged {
        /// Observation sequence position.
        observation_index: usize,
    },
    /// A native-cache observation did not follow the previous local value.
    #[error("native-cache observation {observation_index} has an inconsistent before value")]
    NativeTraceInconsistent {
        /// Observation sequence position.
        observation_index: usize,
    },
}

/// Bounded context and native-cache dependencies/effects for one execution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptimisticHostDependencies {
    contexts: Vec<OptimisticContextDependency>,
    native_cache: Vec<NativeCacheDependency>,
    native_effects: Vec<NativeCacheEffect>,
}

impl OptimisticHostDependencies {
    pub(crate) fn from_journal(
        journal: &ExecutionObservationJournal,
    ) -> Result<Self, HostDependencyCaptureError> {
        let contexts = journal
            .context_observations()
            .iter()
            .enumerate()
            .map(
                |(observation_index, observation)| OptimisticContextDependency {
                    observation_index,
                    access: observation.access,
                    value: OptimisticContextValue::from_snapshot(&observation.value),
                },
            )
            .collect();

        let (native_cache, native_effects) = capture_native_dependencies(journal)?;
        Ok(Self {
            contexts,
            native_cache,
            native_effects,
        })
    }

    /// Context dependencies in exact observation order.
    #[must_use]
    pub fn contexts(&self) -> &[OptimisticContextDependency] {
        &self.contexts
    }

    /// Native-cache dependencies ordered by stable domain and scope identity.
    #[must_use]
    pub fn native_cache(&self) -> &[NativeCacheDependency] {
        &self.native_cache
    }

    /// Native-cache writes in exact observation order.
    #[must_use]
    pub fn native_effects(&self) -> &[NativeCacheEffect] {
        &self.native_effects
    }

    /// Revalidates context observations through a caller-owned lookup.
    pub fn try_revalidate_contexts<E>(
        &self,
        mut lookup: impl FnMut(usize, HostContextAccess) -> Result<OptimisticContextValue, E>,
    ) -> Result<HostDependencyValidation, E> {
        for dependency in &self.contexts {
            let actual = lookup(dependency.observation_index, dependency.access)?;
            if actual != dependency.value {
                return Ok(HostDependencyValidation::ContextConflict {
                    observation_index: dependency.observation_index,
                    access: dependency.access,
                });
            }
        }
        Ok(HostDependencyValidation::Valid {
            contexts: self.contexts.len(),
            native_cache: 0,
        })
    }

    /// Revalidates every captured context and native-cache dependency in one
    /// deterministic pass. Context observations precede native-cache
    /// locations; the first mismatch stops the pass.
    pub fn try_revalidate<E>(
        &self,
        mut context_lookup: impl FnMut(usize, HostContextAccess) -> Result<OptimisticContextValue, E>,
        mut native_lookup: impl FnMut(&NativeCacheLocation) -> Result<Option<Vec<u8>>, E>,
    ) -> Result<HostDependencyValidation, E> {
        for dependency in &self.contexts {
            let actual = context_lookup(dependency.observation_index, dependency.access)?;
            if actual != dependency.value {
                return Ok(HostDependencyValidation::ContextConflict {
                    observation_index: dependency.observation_index,
                    access: dependency.access,
                });
            }
        }
        for (dependency_index, dependency) in self.native_cache.iter().enumerate() {
            let actual = native_lookup(&dependency.location)?;
            if let Some(kind) =
                classify_value_conflict(dependency.expected.as_deref(), actual.as_deref())
            {
                return Ok(HostDependencyValidation::NativeCacheConflict {
                    dependency_index,
                    location: dependency.location.clone(),
                    kind,
                });
            }
        }
        Ok(HostDependencyValidation::Valid {
            contexts: self.contexts.len(),
            native_cache: self.native_cache.len(),
        })
    }

    /// Revalidates native-cache dependencies through a caller-owned lookup.
    pub fn try_revalidate_native_cache<E>(
        &self,
        mut lookup: impl FnMut(&NativeCacheLocation) -> Result<Option<Vec<u8>>, E>,
    ) -> Result<HostDependencyValidation, E> {
        for (dependency_index, dependency) in self.native_cache.iter().enumerate() {
            let actual = lookup(&dependency.location)?;
            if let Some(kind) =
                classify_value_conflict(dependency.expected.as_deref(), actual.as_deref())
            {
                return Ok(HostDependencyValidation::NativeCacheConflict {
                    dependency_index,
                    location: dependency.location.clone(),
                    kind,
                });
            }
        }
        Ok(HostDependencyValidation::Valid {
            contexts: 0,
            native_cache: self.native_cache.len(),
        })
    }
}

fn capture_native_dependencies(
    journal: &ExecutionObservationJournal,
) -> Result<(Vec<NativeCacheDependency>, Vec<NativeCacheEffect>), HostDependencyCaptureError> {
    struct Trace {
        location: NativeCacheLocation,
        expected: Option<Vec<u8>>,
        visible: Option<Vec<u8>>,
    }

    let mut traces: BTreeMap<Vec<u8>, Trace> = BTreeMap::new();
    let mut effects = Vec::new();
    for (observation_index, observation) in journal.native_cache_observations().iter().enumerate() {
        let location = NativeCacheLocation::from_access(&observation.access);
        let sort_key = native_location_sort_key(&location);
        let trace = traces.entry(sort_key).or_insert_with(|| Trace {
            location: location.clone(),
            expected: observation.before.clone(),
            visible: observation.before.clone(),
        });
        if trace.visible != observation.before {
            return Err(HostDependencyCaptureError::NativeTraceInconsistent { observation_index });
        }
        match observation.access.kind() {
            NativeCacheAccessKind::Read => {
                if observation.after != observation.before {
                    return Err(HostDependencyCaptureError::NativeReadChanged {
                        observation_index,
                    });
                }
            }
            NativeCacheAccessKind::Write => {
                effects.push(NativeCacheEffect {
                    observation_index,
                    location: location.clone(),
                    value: observation.after.clone(),
                });
                trace.visible = observation.after.clone();
            }
        }
    }

    let dependencies = traces
        .into_values()
        .map(|trace| NativeCacheDependency {
            location: trace.location,
            expected: trace.expected,
        })
        .collect();
    Ok((dependencies, effects))
}

fn native_location_sort_key(location: &NativeCacheLocation) -> Vec<u8> {
    let domain = location.domain;
    let mut key = Vec::with_capacity(20 + 4 + 4 + 2 + 1 + 32);
    key.extend_from_slice(&domain.contract_hash.to_bytes());
    key.extend_from_slice(&domain.contract_id.to_le_bytes());
    key.extend_from_slice(&domain.native_version.to_le_bytes());
    key.extend_from_slice(&domain.partition.to_le_bytes());
    match &location.scope {
        ResolvedNativeCacheScope::WholeDomain => key.push(0),
        ResolvedNativeCacheScope::Entry(entry) => {
            key.push(1);
            key.extend_from_slice(entry);
        }
    }
    key
}

/// Native-cache conflict classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeCacheConflictKind {
    /// Expected value changed while remaining present.
    PresentValueChanged,
    /// Expected value was deleted.
    PresentDeleted,
    /// Expected absence became present.
    AbsentCreated,
}

/// First host dependency conflict, or valid counts when all checked values match.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostDependencyValidation {
    /// All dependencies checked by this validation pass matched.
    Valid {
        /// Number of context observations checked.
        contexts: usize,
        /// Number of native-cache locations checked.
        native_cache: usize,
    },
    /// One context observation changed.
    ContextConflict {
        /// Exact observation sequence position.
        observation_index: usize,
        /// Context identity.
        access: HostContextAccess,
    },
    /// One native-cache location changed.
    NativeCacheConflict {
        /// Canonical sorted dependency position.
        dependency_index: usize,
        /// Exact versioned location.
        location: NativeCacheLocation,
        /// Present/absent transition or value change.
        kind: NativeCacheConflictKind,
    },
}

impl HostDependencyValidation {
    /// Returns whether this result contains no conflict.
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        matches!(self, Self::Valid { .. })
    }
}

fn classify_value_conflict(
    expected: Option<&[u8]>,
    actual: Option<&[u8]>,
) -> Option<NativeCacheConflictKind> {
    match (expected, actual) {
        (Some(expected), Some(actual)) if expected != actual => {
            Some(NativeCacheConflictKind::PresentValueChanged)
        }
        (Some(_), None) => Some(NativeCacheConflictKind::PresentDeleted),
        (None, Some(_)) => Some(NativeCacheConflictKind::AbsentCreated),
        (Some(_), Some(_)) | (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_artifact::{ContextObservationValue, ExecutionArtifactLimits};
    use crate::host_access_audit::ResolvedNativeCacheScope;
    use neo_primitives::{TriggerType, UInt160};
    use neo_vm::NativeCacheDomain;
    use neo_vm::StackItem;

    fn domain(partition: u16) -> NativeCacheDomain {
        NativeCacheDomain {
            contract_hash: UInt160::zero(),
            contract_id: 1,
            native_version: 3,
            partition,
        }
    }

    fn access(
        partition: u16,
        scope: ResolvedNativeCacheScope,
        kind: NativeCacheAccessKind,
    ) -> NativeCacheAccess {
        NativeCacheAccess::new(domain(partition), scope, kind)
    }

    #[test]
    fn context_snapshot_preserves_sequence_and_exact_values() {
        let mut journal =
            ExecutionObservationJournal::with_limits(ExecutionArtifactLimits::default());
        journal
            .record_context(
                HostContextAccess::Trigger,
                ContextObservationValue::Trigger(TriggerType::Application),
            )
            .expect("context");
        journal
            .record_context(
                HostContextAccess::BlockIndex,
                ContextObservationValue::U32(42),
            )
            .expect("context");
        let dependencies = OptimisticHostDependencies::from_journal(&journal).expect("snapshot");
        assert_eq!(dependencies.contexts().len(), 2);
        assert_eq!(dependencies.contexts()[0].observation_index(), 0);
        assert_eq!(
            dependencies.contexts()[0].access(),
            HostContextAccess::Trigger
        );
        assert_eq!(
            dependencies.try_revalidate_contexts(|_, access| {
                Ok::<_, ()>(match access {
                    HostContextAccess::Trigger => {
                        OptimisticContextValue::Trigger(TriggerType::Application)
                    }
                    HostContextAccess::BlockIndex => OptimisticContextValue::U32(42),
                    _ => unreachable!(),
                })
            }),
            Ok(HostDependencyValidation::Valid {
                contexts: 2,
                native_cache: 0,
            })
        );
    }

    #[test]
    fn first_context_conflict_is_deterministic() {
        let mut journal = ExecutionObservationJournal::new();
        journal
            .record_context(HostContextAccess::Network, ContextObservationValue::U32(1))
            .expect("context");
        journal
            .record_context(
                HostContextAccess::BlockIndex,
                ContextObservationValue::U32(2),
            )
            .expect("context");
        let dependencies = OptimisticHostDependencies::from_journal(&journal).expect("snapshot");
        let mut visited = Vec::new();
        let result = dependencies
            .try_revalidate_contexts(|index, _access| {
                visited.push(index);
                Ok::<_, ()>(OptimisticContextValue::U32(99))
            })
            .expect("validation");
        assert_eq!(visited, vec![0]);
        assert_eq!(
            result,
            HostDependencyValidation::ContextConflict {
                observation_index: 0,
                access: HostContextAccess::Network,
            }
        );
    }

    #[test]
    fn native_dependencies_deduplicate_prefix_reads_and_retain_write_effects() {
        let location = access(
            7,
            ResolvedNativeCacheScope::Entry(b"balance".to_vec()),
            NativeCacheAccessKind::Write,
        );
        let read = NativeCacheAccess::new(
            location.domain(),
            location.scope().clone(),
            NativeCacheAccessKind::Read,
        );
        let mut journal = ExecutionObservationJournal::new();
        journal
            .record_native_cache(location, Some(b"old".to_vec()), Some(b"new".to_vec()))
            .expect("write");
        journal
            .record_native_cache(read, Some(b"new".to_vec()), Some(b"new".to_vec()))
            .expect("read");
        let dependencies = OptimisticHostDependencies::from_journal(&journal).expect("snapshot");
        assert_eq!(dependencies.native_cache().len(), 1);
        assert_eq!(dependencies.native_cache()[0].expected(), Some(&b"old"[..]));
        assert_eq!(dependencies.native_effects().len(), 1);
        assert_eq!(dependencies.native_effects()[0].value(), Some(&b"new"[..]));
    }

    #[test]
    fn native_conflicts_classify_changed_deleted_and_created_values() {
        for (expected, actual, kind) in [
            (
                Some(b"old".to_vec()),
                Some(b"new".to_vec()),
                NativeCacheConflictKind::PresentValueChanged,
            ),
            (
                Some(b"old".to_vec()),
                None,
                NativeCacheConflictKind::PresentDeleted,
            ),
            (
                None,
                Some(b"new".to_vec()),
                NativeCacheConflictKind::AbsentCreated,
            ),
        ] {
            let mut journal = ExecutionObservationJournal::new();
            journal
                .record_native_cache(
                    access(
                        1,
                        ResolvedNativeCacheScope::WholeDomain,
                        NativeCacheAccessKind::Read,
                    ),
                    expected.clone(),
                    expected.clone(),
                )
                .expect("native read");
            let dependencies =
                OptimisticHostDependencies::from_journal(&journal).expect("snapshot");
            let result = dependencies
                .try_revalidate_native_cache(|_| Ok::<_, ()>(actual.clone()))
                .expect("validation");
            assert_eq!(
                result,
                HostDependencyValidation::NativeCacheConflict {
                    dependency_index: 0,
                    location: dependencies.native_cache()[0].location().clone(),
                    kind,
                }
            );
        }
    }

    #[test]
    fn inconsistent_native_trace_fails_closed() {
        let location = access(
            1,
            ResolvedNativeCacheScope::Entry(b"x".to_vec()),
            NativeCacheAccessKind::Write,
        );
        let read = NativeCacheAccess::new(
            location.domain(),
            location.scope().clone(),
            NativeCacheAccessKind::Read,
        );
        let mut journal = ExecutionObservationJournal::new();
        journal
            .record_native_cache(location, Some(b"old".to_vec()), Some(b"new".to_vec()))
            .expect("write");
        journal
            .record_native_cache(read, Some(b"wrong".to_vec()), Some(b"wrong".to_vec()))
            .expect("read");
        assert_eq!(
            OptimisticHostDependencies::from_journal(&journal),
            Err(HostDependencyCaptureError::NativeTraceInconsistent {
                observation_index: 1,
            })
        );
    }

    #[test]
    fn stack_contexts_are_retained_as_canonical_documents() {
        let mut journal = ExecutionObservationJournal::new();
        journal
            .record_context(
                HostContextAccess::Notifications,
                ContextObservationValue::StackItems(vec![StackItem::from_int(7)]),
            )
            .expect("context");
        let dependencies = OptimisticHostDependencies::from_journal(&journal).expect("snapshot");
        assert!(matches!(
            dependencies.contexts()[0].value(),
            OptimisticContextValue::StackItems(_)
        ));
    }
}
