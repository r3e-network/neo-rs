use super::bounds::{ExecutionArtifactError, ExecutionArtifactLimits};
use super::stack::CanonicalStackDocument;
use crate::host_access_audit::{
    ContractCallAccess, HostContextAccess, NativeCacheAccess, ResolvedNativeCacheScope,
    ResolvedStorageRangeDomain, StorageRangeAccess,
};
use neo_primitives::{TriggerType, UInt160, UInt256};
use neo_storage::{StorageItem, StorageKey};
use neo_vm::StackItem;
use std::collections::HashMap;
use std::sync::Arc;

/// Completed contract-call outcome supplied to the observation journal.
#[derive(Clone, Debug)]
pub enum CallObservationOutcome {
    /// The call returned these stack items in order.
    Returned(Vec<StackItem>),
    /// The call faulted with an exact message and optional exception value.
    Fault {
        /// Fault message exposed by the execution host.
        message: String,
        /// VM exception value, when available.
        exception: Option<StackItem>,
    },
}

/// Exact value supplied for one observed execution-context dependency.
#[derive(Clone, Debug)]
pub enum ContextObservationValue {
    /// Boolean value.
    Boolean(bool),
    /// Unsigned byte.
    U8(u8),
    /// Unsigned 32-bit value.
    U32(u32),
    /// Unsigned 64-bit value.
    U64(u64),
    /// Signed 64-bit value.
    I64(i64),
    /// Trigger flags.
    Trigger(TriggerType),
    /// Effective call flags encoded as their exact byte.
    CallFlags(u8),
    /// Optional script hash.
    Hash160(Option<UInt160>),
    /// Optional script-container hash.
    Hash256(Option<UInt256>),
    /// Stack items returned by a context query such as `GetNotifications`.
    StackItems(Vec<StackItem>),
}

/// Witness check completion retained by the observation journal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WitnessObservationOutcome {
    /// The check returned a boolean decision.
    Returned(bool),
    /// The check faulted.
    Fault(String),
}

/// Diagnostic callback kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticObservationKind {
    /// Engine initialization callback.
    Initialized,
    /// Context-load callback.
    ContextLoaded,
    /// Pre-instruction callback.
    PreInstruction,
    /// Post-instruction callback.
    PostInstruction,
    /// Context-unload callback.
    ContextUnloaded,
    /// Engine disposal callback.
    Disposed,
}

#[derive(Clone, Debug)]
pub(super) struct StoragePointObservation {
    pub(super) key: StorageKey,
    pub(super) value: Option<Vec<u8>>,
}

#[derive(Clone, Debug)]
pub(super) struct StorageRangeObservation {
    pub(super) access: StorageRangeAccess,
    pub(super) rows: Vec<(StorageKey, Vec<u8>)>,
}

#[derive(Clone, Debug)]
pub(crate) struct NativeCacheObservation {
    pub(crate) access: NativeCacheAccess,
    pub(crate) before: Option<Vec<u8>>,
    pub(crate) after: Option<Vec<u8>>,
}

#[derive(Clone, Debug)]
pub(super) struct ContractCallObservation {
    pub(super) access: ContractCallAccess,
    pub(super) argument_count: usize,
    pub(super) stack: Arc<CanonicalStackDocument>,
    pub(super) outcome: CallObservationSnapshotOutcome,
}

#[derive(Clone, Debug)]
pub(super) enum CallObservationSnapshotOutcome {
    Returned {
        value_count: usize,
    },
    Fault {
        message: String,
        has_exception: bool,
    },
}

#[derive(Clone, Debug)]
pub(super) struct WitnessObservation {
    pub(super) hash: UInt160,
    pub(super) outcome: WitnessObservationOutcome,
}

#[derive(Clone, Debug)]
pub(crate) struct ContextObservation {
    pub(crate) access: HostContextAccess,
    pub(crate) value: ContextObservationSnapshotValue,
}

#[derive(Clone, Debug)]
pub(crate) enum ContextObservationSnapshotValue {
    Boolean(bool),
    U8(u8),
    U32(u32),
    U64(u64),
    I64(i64),
    Trigger(TriggerType),
    CallFlags(u8),
    Hash160(Option<UInt160>),
    Hash256(Option<UInt256>),
    StackItems(Arc<CanonicalStackDocument>),
}

#[derive(Clone, Debug)]
pub(super) struct DiagnosticObservation {
    pub(super) kind: DiagnosticObservationKind,
    pub(super) script_hash: Option<UInt160>,
    pub(super) instruction_pointer: Option<u64>,
    pub(super) instruction: Vec<u8>,
    pub(super) stack: Arc<CanonicalStackDocument>,
}

/// Exact observations that are not recoverable from final engine state alone.
///
/// The shadow runner records through this journal only when explicitly enabled.
/// Every insertion is bounded before the journal retains it. Stack-bearing
/// observations are canonicalized immediately, preserving aliases and cycles
/// without retaining mutable VM object handles. Final artifact capture applies
/// the same bounds independently across the journal and final engine state.
#[derive(Clone, Debug)]
pub struct ExecutionObservationJournal {
    limits: ExecutionArtifactLimits,
    retained_bytes: usize,
    stack_roots: usize,
    stack_nodes: usize,
    stack_edges: usize,
    pub(super) storage_reads: Vec<StoragePointObservation>,
    pub(super) storage_ranges: Vec<StorageRangeObservation>,
    storage_range_rows: usize,
    pub(super) native_cache: Vec<NativeCacheObservation>,
    pub(super) calls: Vec<ContractCallObservation>,
    pub(super) witnesses: Vec<WitnessObservation>,
    pub(super) contexts: Vec<ContextObservation>,
    pub(super) fee_charges: Vec<u64>,
    pub(super) diagnostics: Vec<DiagnosticObservation>,
}

#[derive(Clone, Debug)]
struct PendingCallObservation {
    sequence: u64,
    access: ContractCallAccess,
    arguments: Vec<StackItem>,
}

/// Runner-owned mutable state for live shadow observations.
///
/// The first observation failure is latched and all later observations become
/// no-ops. This keeps retained memory bounded while allowing the ordinary VM to
/// finish with its unmodified semantic result. Artifact capture then propagates
/// the exact failure and fail-closes the optimized comparison.
#[derive(Debug)]
pub(crate) struct ExecutionObservationState {
    journal: ExecutionObservationJournal,
    failure: Option<ExecutionArtifactError>,
    pending_calls: HashMap<usize, PendingCallObservation>,
    next_call_sequence: u64,
}

impl ExecutionObservationState {
    #[must_use]
    pub(crate) fn new(limits: ExecutionArtifactLimits) -> Self {
        Self {
            journal: ExecutionObservationJournal::with_limits(limits),
            failure: None,
            pending_calls: HashMap::new(),
            next_call_sequence: 0,
        }
    }

    pub(crate) fn record(
        &mut self,
        observation: impl FnOnce(&mut ExecutionObservationJournal) -> Result<(), ExecutionArtifactError>,
    ) {
        if self.failure.is_some() {
            return;
        }
        if let Err(error) = observation(&mut self.journal) {
            self.failure = Some(error);
        }
    }

    pub(crate) fn fail(&mut self, error: ExecutionArtifactError) {
        if self.failure.is_none() {
            self.failure = Some(error);
        }
    }

    pub(crate) fn begin_call(
        &mut self,
        context_identity: usize,
        access: ContractCallAccess,
        arguments: Vec<StackItem>,
    ) {
        if self.failure.is_some() {
            return;
        }
        let actual = self
            .journal
            .calls
            .len()
            .saturating_add(self.pending_calls.len())
            .saturating_add(1);
        if actual > self.journal.limits.max_calls {
            self.fail(ExecutionArtifactError::LimitExceeded {
                resource: "calls",
                actual,
                maximum: self.journal.limits.max_calls,
            });
            return;
        }
        let sequence = self.next_call_sequence;
        self.next_call_sequence = self.next_call_sequence.saturating_add(1);
        if self
            .pending_calls
            .insert(
                context_identity,
                PendingCallObservation {
                    sequence,
                    access,
                    arguments,
                },
            )
            .is_some()
        {
            self.fail(ExecutionArtifactError::LimitExceeded {
                resource: "pending call identities",
                actual: 2,
                maximum: 1,
            });
        }
    }

    pub(crate) fn retarget_call(
        &mut self,
        context_identity: usize,
        kind: crate::host_access_audit::ContractCallKind,
        native_calling_script_hash: Option<UInt160>,
    ) {
        let Some(pending) = self.pending_calls.get_mut(&context_identity) else {
            return;
        };
        let current = &pending.access;
        let mut replacement = ContractCallAccess::new(
            kind,
            current.contract(),
            current.entry_ip(),
            current.method(),
            current.call_flags(),
            current.argument_count(),
            current.result_count(),
        );
        if let Some(hash) = native_calling_script_hash {
            replacement = replacement.with_native_calling_script_hash(hash);
        }
        pending.access = replacement;
    }

    pub(crate) fn complete_call(
        &mut self,
        context_identity: usize,
        outcome: CallObservationOutcome,
    ) {
        if self.failure.is_some() {
            return;
        }
        let Some(pending) = self.pending_calls.remove(&context_identity) else {
            return;
        };
        self.record(|journal| journal.record_call(pending.access, pending.arguments, outcome));
    }

    pub(crate) fn complete_all_calls(&mut self, outcome: CallObservationOutcome) {
        if self.failure.is_some() {
            return;
        }
        let mut pending = std::mem::take(&mut self.pending_calls)
            .into_values()
            .collect::<Vec<_>>();
        pending.sort_unstable_by_key(|call| std::cmp::Reverse(call.sequence));
        for call in pending {
            let outcome = outcome.clone();
            self.record(|journal| journal.record_call(call.access, call.arguments, outcome));
        }
    }

    pub(crate) fn journal(&self) -> Result<&ExecutionObservationJournal, ExecutionArtifactError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        Ok(&self.journal)
    }
}

impl ExecutionObservationJournal {
    /// Creates an empty journal for a pure candidate or an execution with no
    /// observed host dependencies.
    #[must_use]
    pub const fn new() -> Self {
        Self::with_limits(ExecutionArtifactLimits::DEFAULT)
    }

    /// Creates an empty journal with explicit observation-time hard bounds.
    #[must_use]
    pub const fn with_limits(limits: ExecutionArtifactLimits) -> Self {
        Self {
            limits,
            retained_bytes: 0,
            stack_roots: 0,
            stack_nodes: 0,
            stack_edges: 0,
            storage_reads: Vec::new(),
            storage_ranges: Vec::new(),
            storage_range_rows: 0,
            native_cache: Vec::new(),
            calls: Vec::new(),
            witnesses: Vec::new(),
            contexts: Vec::new(),
            fee_charges: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    /// Returns bounded native-cache observations for an opt-in execution
    /// dependency snapshot. The caller must not mutate the journal while
    /// iterating this slice.
    pub(crate) fn native_cache_observations(&self) -> &[NativeCacheObservation] {
        &self.native_cache
    }

    /// Returns bounded context observations for an opt-in execution
    /// dependency snapshot. The caller must not mutate the journal while
    /// iterating this slice.
    pub(crate) fn context_observations(&self) -> &[ContextObservation] {
        &self.contexts
    }

    /// Records one exact present or absent point read.
    pub fn record_storage_read(
        &mut self,
        key: StorageKey,
        value: Option<Vec<u8>>,
    ) -> Result<(), ExecutionArtifactError> {
        self.require_next(
            self.storage_reads.len(),
            self.limits.max_storage_reads,
            "storage read observations",
        )?;
        let bytes =
            storage_key_retained_bytes(&key).saturating_add(value.as_ref().map_or(0, Vec::len));
        self.commit_plain_bytes(bytes)?;
        self.storage_reads
            .push(StoragePointObservation { key, value });
        Ok(())
    }

    pub(crate) fn record_storage_read_borrowed(
        &mut self,
        key: &StorageKey,
        value: Option<&StorageItem>,
    ) -> Result<(), ExecutionArtifactError> {
        self.require_next(
            self.storage_reads.len(),
            self.limits.max_storage_reads,
            "storage read observations",
        )?;
        let bytes = storage_key_retained_bytes(key)
            .saturating_add(value.map_or(0, |item| item.value_bytes().len()));
        self.commit_plain_bytes(bytes)?;
        self.storage_reads.push(StoragePointObservation {
            key: key.clone(),
            value: value.map(|item| item.value_bytes().into_owned()),
        });
        Ok(())
    }

    /// Records one exact range result in traversal order.
    pub fn record_storage_range(
        &mut self,
        access: StorageRangeAccess,
        rows: Vec<(StorageKey, Vec<u8>)>,
    ) -> Result<(), ExecutionArtifactError> {
        self.require_next(
            self.storage_ranges.len(),
            self.limits.max_storage_ranges,
            "storage range observations",
        )?;
        let range_rows_after = self.storage_range_rows_after(rows.len(), access.max_items())?;
        let mut bytes = storage_range_access_bytes(&access);
        for (key, value) in &rows {
            bytes = bytes
                .saturating_add(storage_key_retained_bytes(key))
                .saturating_add(value.len());
        }
        self.commit_plain_bytes(bytes)?;
        self.storage_ranges
            .push(StorageRangeObservation { access, rows });
        self.storage_range_rows = range_rows_after;
        Ok(())
    }

    pub(crate) fn record_storage_range_borrowed(
        &mut self,
        access: &StorageRangeAccess,
        rows: &[(StorageKey, StorageItem)],
    ) -> Result<(), ExecutionArtifactError> {
        self.require_next(
            self.storage_ranges.len(),
            self.limits.max_storage_ranges,
            "storage range observations",
        )?;
        let range_rows_after = self.storage_range_rows_after(rows.len(), access.max_items())?;
        let mut bytes = storage_range_access_bytes(access);
        for (key, value) in rows {
            bytes = bytes
                .saturating_add(storage_key_retained_bytes(key))
                .saturating_add(value.value_bytes().len());
        }
        self.commit_plain_bytes(bytes)?;
        self.storage_ranges.push(StorageRangeObservation {
            access: access.clone(),
            rows: rows
                .iter()
                .map(|(key, value)| (key.clone(), value.value_bytes().into_owned()))
                .collect(),
        });
        self.storage_range_rows = range_rows_after;
        Ok(())
    }

    pub(crate) fn refine_last_storage_range_access(
        &mut self,
        access: StorageRangeAccess,
    ) -> Result<(), ExecutionArtifactError> {
        let Some(observation) = self.storage_ranges.last_mut() else {
            return Err(ExecutionArtifactError::ObservationFailed {
                kind: "storage range metadata",
                message: "host metadata has no preceding cache range observation".to_string(),
            });
        };
        if observation.access.contract_id() != access.contract_id()
            || observation.access.domain() != access.domain()
            || observation.access.direction() != access.direction()
        {
            return Err(ExecutionArtifactError::ObservationFailed {
                kind: "storage range metadata",
                message: "host metadata does not match the preceding cache range".to_string(),
            });
        }
        Self::require_count(
            observation.rows.len(),
            access.max_items() as usize,
            "storage range rows",
        )?;
        observation.access = access;
        Ok(())
    }

    /// Records one versioned native-cache read or write observation.
    pub fn record_native_cache(
        &mut self,
        access: NativeCacheAccess,
        before: Option<Vec<u8>>,
        after: Option<Vec<u8>>,
    ) -> Result<(), ExecutionArtifactError> {
        self.require_next(
            self.native_cache.len(),
            self.limits.max_native_cache_observations,
            "native-cache observations",
        )?;
        let scope_bytes = match access.scope() {
            ResolvedNativeCacheScope::Entry(key) => key.len(),
            ResolvedNativeCacheScope::WholeDomain => 0,
        };
        let bytes = scope_bytes
            .saturating_add(before.as_ref().map_or(0, Vec::len))
            .saturating_add(after.as_ref().map_or(0, Vec::len));
        self.commit_plain_bytes(bytes)?;
        self.native_cache.push(NativeCacheObservation {
            access,
            before,
            after,
        });
        Ok(())
    }

    /// Records one completed declared contract call.
    pub fn record_call(
        &mut self,
        access: ContractCallAccess,
        arguments: Vec<StackItem>,
        outcome: CallObservationOutcome,
    ) -> Result<(), ExecutionArtifactError> {
        self.require_next(self.calls.len(), self.limits.max_calls, "calls")?;
        let argument_count = arguments.len();
        let method_bytes = access.method().len();
        let (stack, outcome, reserved_bytes) = match outcome {
            CallObservationOutcome::Returned(values) => {
                let stack =
                    self.capture_stack(&[arguments.as_slice(), values.as_slice()], method_bytes)?;
                (
                    stack,
                    CallObservationSnapshotOutcome::Returned {
                        value_count: values.len(),
                    },
                    method_bytes,
                )
            }
            CallObservationOutcome::Fault { message, exception } => {
                let exception_slice = exception
                    .as_ref()
                    .map_or([].as_slice(), std::slice::from_ref);
                let reserved_bytes = method_bytes.saturating_add(message.len());
                let stack =
                    self.capture_stack(&[arguments.as_slice(), exception_slice], reserved_bytes)?;
                (
                    stack,
                    CallObservationSnapshotOutcome::Fault {
                        message,
                        has_exception: !exception_slice.is_empty(),
                    },
                    reserved_bytes,
                )
            }
        };
        self.commit_stack(&stack, reserved_bytes);
        self.calls.push(ContractCallObservation {
            access,
            argument_count,
            stack,
            outcome,
        });
        Ok(())
    }

    /// Records one witness check and its exact outcome.
    pub fn record_witness(
        &mut self,
        hash: UInt160,
        outcome: WitnessObservationOutcome,
    ) -> Result<(), ExecutionArtifactError> {
        self.require_next(
            self.witnesses.len(),
            self.limits.max_witnesses,
            "witness observations",
        )?;
        let bytes = match &outcome {
            WitnessObservationOutcome::Returned(_) => 0,
            WitnessObservationOutcome::Fault(message) => message.len(),
        };
        self.commit_plain_bytes(bytes)?;
        self.witnesses.push(WitnessObservation { hash, outcome });
        Ok(())
    }

    /// Records one declared context dependency and returned value.
    pub fn record_context(
        &mut self,
        access: HostContextAccess,
        value: ContextObservationValue,
    ) -> Result<(), ExecutionArtifactError> {
        self.require_next(
            self.contexts.len(),
            self.limits.max_context_observations,
            "context observations",
        )?;
        let value = match value {
            ContextObservationValue::Boolean(value) => {
                ContextObservationSnapshotValue::Boolean(value)
            }
            ContextObservationValue::U8(value) => ContextObservationSnapshotValue::U8(value),
            ContextObservationValue::U32(value) => ContextObservationSnapshotValue::U32(value),
            ContextObservationValue::U64(value) => ContextObservationSnapshotValue::U64(value),
            ContextObservationValue::I64(value) => ContextObservationSnapshotValue::I64(value),
            ContextObservationValue::Trigger(value) => {
                ContextObservationSnapshotValue::Trigger(value)
            }
            ContextObservationValue::CallFlags(value) => {
                ContextObservationSnapshotValue::CallFlags(value)
            }
            ContextObservationValue::Hash160(value) => {
                ContextObservationSnapshotValue::Hash160(value)
            }
            ContextObservationValue::Hash256(value) => {
                ContextObservationSnapshotValue::Hash256(value)
            }
            ContextObservationValue::StackItems(values) => {
                let stack = self.capture_stack(&[values.as_slice()], 0)?;
                self.commit_stack(&stack, 0);
                ContextObservationSnapshotValue::StackItems(stack)
            }
        };
        self.contexts.push(ContextObservation { access, value });
        Ok(())
    }

    /// Records one exact fee charge in datoshi.
    pub fn record_fee_charge(&mut self, fee: u64) -> Result<(), ExecutionArtifactError> {
        self.require_next(
            self.fee_charges.len(),
            self.limits.max_fee_charges,
            "fee-charge observations",
        )?;
        self.fee_charges.push(fee);
        Ok(())
    }

    /// Records one diagnostic callback, including any diagnostic stack roots.
    pub fn record_diagnostic(
        &mut self,
        kind: DiagnosticObservationKind,
        script_hash: Option<UInt160>,
        instruction_pointer: Option<u64>,
        instruction: Vec<u8>,
        stack: Vec<StackItem>,
    ) -> Result<(), ExecutionArtifactError> {
        self.require_next(
            self.diagnostics.len(),
            self.limits.max_diagnostics,
            "diagnostic observations",
        )?;
        let stack = self.capture_stack(&[stack.as_slice()], instruction.len())?;
        self.commit_stack(&stack, instruction.len());
        self.diagnostics.push(DiagnosticObservation {
            kind,
            script_hash,
            instruction_pointer,
            instruction,
            stack,
        });
        Ok(())
    }

    fn require_next(
        &self,
        current: usize,
        maximum: usize,
        resource: &'static str,
    ) -> Result<(), ExecutionArtifactError> {
        Self::require_count(current.saturating_add(1), maximum, resource)
    }

    fn require_count(
        actual: usize,
        maximum: usize,
        resource: &'static str,
    ) -> Result<(), ExecutionArtifactError> {
        if actual > maximum {
            return Err(ExecutionArtifactError::LimitExceeded {
                resource,
                actual,
                maximum,
            });
        }
        Ok(())
    }

    fn retained_after(&self, bytes: usize) -> Result<usize, ExecutionArtifactError> {
        let actual = self.retained_bytes.checked_add(bytes).ok_or(
            ExecutionArtifactError::LimitExceeded {
                resource: "retained bytes",
                actual: usize::MAX,
                maximum: self.limits.max_retained_bytes,
            },
        )?;
        Self::require_count(actual, self.limits.max_retained_bytes, "retained bytes")?;
        Ok(actual)
    }

    fn storage_range_rows_after(
        &self,
        additional: usize,
        access_maximum: u32,
    ) -> Result<usize, ExecutionArtifactError> {
        Self::require_count(additional, access_maximum as usize, "storage range rows")?;
        let actual = self.storage_range_rows.checked_add(additional).ok_or(
            ExecutionArtifactError::LimitExceeded {
                resource: "storage range rows",
                actual: usize::MAX,
                maximum: self.limits.max_storage_range_rows,
            },
        )?;
        Self::require_count(
            actual,
            self.limits.max_storage_range_rows,
            "storage range rows",
        )?;
        Ok(actual)
    }

    fn commit_plain_bytes(&mut self, bytes: usize) -> Result<(), ExecutionArtifactError> {
        self.retained_bytes = self.retained_after(bytes)?;
        Ok(())
    }

    fn capture_stack(
        &self,
        segments: &[&[StackItem]],
        reserved_bytes: usize,
    ) -> Result<Arc<CanonicalStackDocument>, ExecutionArtifactError> {
        let retained_after_reserved = self.retained_after(reserved_bytes)?;
        let limits = ExecutionArtifactLimits {
            max_retained_bytes: self
                .limits
                .max_retained_bytes
                .saturating_sub(retained_after_reserved),
            max_stack_roots: self.limits.max_stack_roots.saturating_sub(self.stack_roots),
            max_stack_nodes: self.limits.max_stack_nodes.saturating_sub(self.stack_nodes),
            max_stack_edges: self.limits.max_stack_edges.saturating_sub(self.stack_edges),
            ..self.limits
        };
        CanonicalStackDocument::capture_segments(segments.iter().copied(), limits)
            .map(Arc::new)
            .map_err(|error| self.translate_stack_error(error, retained_after_reserved))
    }

    fn translate_stack_error(
        &self,
        error: ExecutionArtifactError,
        retained_after_reserved: usize,
    ) -> ExecutionArtifactError {
        let ExecutionArtifactError::LimitExceeded {
            resource,
            actual,
            maximum,
        } = error
        else {
            return error;
        };
        let (base, maximum) = match resource {
            "retained bytes" => (retained_after_reserved, self.limits.max_retained_bytes),
            "stack roots" => (self.stack_roots, self.limits.max_stack_roots),
            "stack graph nodes" => (self.stack_nodes, self.limits.max_stack_nodes),
            "stack graph edges" => (self.stack_edges, self.limits.max_stack_edges),
            "stack graph depth" => (0, self.limits.max_stack_depth),
            _ => (0, maximum),
        };
        ExecutionArtifactError::LimitExceeded {
            resource,
            actual: base.saturating_add(actual),
            maximum,
        }
    }

    fn commit_stack(&mut self, stack: &CanonicalStackDocument, reserved_bytes: usize) {
        let usage = stack.usage();
        self.retained_bytes = self
            .retained_bytes
            .saturating_add(reserved_bytes)
            .saturating_add(usage.retained_bytes);
        self.stack_roots = self.stack_roots.saturating_add(usage.roots);
        self.stack_nodes = self.stack_nodes.saturating_add(usage.nodes);
        self.stack_edges = self.stack_edges.saturating_add(usage.edges);
    }
}

impl Default for ExecutionObservationJournal {
    fn default() -> Self {
        Self::new()
    }
}

fn storage_range_access_bytes(access: &StorageRangeAccess) -> usize {
    match access.domain() {
        ResolvedStorageRangeDomain::WholeStore => 0,
        ResolvedStorageRangeDomain::Prefix(prefix) => prefix.len(),
        ResolvedStorageRangeDomain::HalfOpen { start, end } => {
            start.len().saturating_add(end.len())
        }
    }
}

fn storage_key_retained_bytes(key: &StorageKey) -> usize {
    std::mem::size_of::<i32>().saturating_add(key.key().len())
}
