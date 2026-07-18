use super::observation::{DiagnosticObservationKind, WitnessObservationOutcome};
use super::stack::{CanonicalStackDocument, CanonicalStackGraph, CanonicalStackValue};
use crate::host_access_audit::{
    ContractCallAccess, HostContextAccess, NativeCacheAccess, StorageRangeAccess,
};
use neo_primitives::{ContractParameterType, Hardfork, TriggerType, UInt160, UInt256};
use neo_vm::{ContractResolutionIdentity, ExceptionHandlingState, VmState};
use std::sync::Arc;

/// Consensus-relevant protocol environment captured with an artifact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolEnvironmentArtifact {
    network: u32,
    address_version: u8,
    standby_committee: Vec<Vec<u8>>,
    validators_count: i32,
    milliseconds_per_block: u32,
    max_valid_until_block_increment: u32,
    max_transactions_per_block: u32,
    max_block_size: u32,
    max_traceable_blocks: u32,
    hardforks: Vec<(Hardfork, u32)>,
    initial_gas_distribution: u64,
}

/// Execution environment and mutable host counters outside the VM stack graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionEnvironmentArtifact {
    trigger: TriggerType,
    protocol: ProtocolEnvironmentArtifact,
    current_block_index: u32,
    persisting_block_hash: Option<UInt256>,
    persisting_block_timestamp: Option<u64>,
    script_container_hash: Option<UInt256>,
    current_script_hash: Option<UInt160>,
    calling_script_hash: Option<UInt160>,
    entry_script_hash: Option<UInt160>,
    call_flags: u8,
    fee_limit_pico: i64,
    exec_fee_factor: u32,
    storage_price: u32,
    random_times: u32,
    nonce_data: [u8; 16],
    native_calling_override: Option<UInt160>,
    native_argument_null_mask: u32,
    native_return_is_null: bool,
    next_iterator_id: u32,
    vm_gas_consumed: u64,
    vm_gas_limit: u64,
    vm_is_jumping: bool,
    vm_call_flags: u8,
}

/// Exact try/catch/finally frame state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalExceptionFrame {
    catch_pointer: i32,
    finally_pointer: i32,
    end_pointer: i32,
    state: ExceptionHandlingState,
}

/// Link to the calling context retained by application context state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallingContextArtifact {
    script: u32,
    instruction_pointer: u64,
    script_hash: Option<UInt160>,
    calling_script_hash: Option<UInt160>,
    native_calling_script_hash: Option<UInt160>,
    has_calling_context: bool,
}

/// Host state attached to one active VM invocation frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationContextStateArtifact {
    script_hash: Option<UInt160>,
    calling_script_hash: Option<UInt160>,
    calling_context: Option<CallingContextArtifact>,
    native_calling_script_hash: Option<UInt160>,
    contract: Option<ContractResolutionIdentity>,
    call_flags: u8,
    snapshot_changes: Vec<CanonicalStorageChange>,
    notification_count: usize,
    is_dynamic_call: bool,
    whitelisted: bool,
    method_name: Option<String>,
    argument_count: usize,
    return_type: Option<ContractParameterType>,
    parameter_types: Vec<ContractParameterType>,
}

/// Complete active invocation frame snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalInvocationFrame {
    script: u32,
    instruction_pointer: u64,
    return_value_count: i32,
    evaluation_stack_group: u32,
    static_fields_group: u32,
    state_group: u32,
    native_call_boundary: bool,
    evaluation_stack: Vec<CanonicalStackValue>,
    static_fields: Option<Vec<CanonicalStackValue>>,
    local_variables: Option<Vec<CanonicalStackValue>>,
    arguments: Option<Vec<CanonicalStackValue>>,
    try_stack: Vec<CanonicalExceptionFrame>,
    application: ApplicationContextStateArtifact,
}

/// Canonical storage put or delete.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalStorageChange {
    key: Vec<u8>,
    value: Option<Vec<u8>>,
}

/// Canonical exact point-read observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalStoragePointObservation {
    key: Vec<u8>,
    value: Option<Vec<u8>>,
}

/// Canonical exact range-read observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalStorageRangeObservation {
    access: StorageRangeAccess,
    rows: Vec<(Vec<u8>, Vec<u8>)>,
}

/// Canonical native-cache observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalNativeCacheObservation {
    access: NativeCacheAccess,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
}

/// Canonical contract-call outcome.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CanonicalCallOutcome {
    /// Returned values.
    Returned {
        /// Number of returned roots after the argument roots.
        value_count: usize,
    },
    /// Fault and optional exact VM exception.
    Fault {
        /// Exposed message.
        message: String,
        /// Whether the last root is an exception value.
        has_exception: bool,
    },
}

/// Canonical completed contract call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalCallObservation {
    access: ContractCallAccess,
    argument_count: usize,
    stack: Arc<CanonicalStackDocument>,
    outcome: CanonicalCallOutcome,
}

/// Native call still queued when execution stopped.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalPendingNativeCall {
    calling_script_hash: UInt160,
    contract_hash: UInt160,
    method: String,
    arguments: Vec<CanonicalStackValue>,
}

/// Canonical context dependency result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CanonicalContextObservationValue {
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
    /// Effective call flags.
    CallFlags(u8),
    /// Optional script hash.
    Hash160(Option<UInt160>),
    /// Optional container hash.
    Hash256(Option<UInt256>),
    /// Canonical stack roots.
    StackItems(Arc<CanonicalStackDocument>),
}

/// Canonical context observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalContextObservation {
    access: HostContextAccess,
    value: CanonicalContextObservationValue,
}

/// Canonical witness observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalWitnessObservation {
    hash: UInt160,
    outcome: WitnessObservationOutcome,
}

/// Canonical notification artifact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalNotification {
    script_container_hash: Option<UInt256>,
    script_hash: UInt160,
    event_name: String,
    state: Vec<CanonicalStackValue>,
    state_array: CanonicalStackValue,
}

/// Canonical log artifact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalLog {
    script_container_hash: Option<UInt256>,
    script_hash: UInt160,
    message: String,
}

/// Canonical diagnostic callback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalDiagnosticObservation {
    kind: DiagnosticObservationKind,
    script_hash: Option<UInt160>,
    instruction_pointer: Option<u64>,
    instruction: Vec<u8>,
    stack: Arc<CanonicalStackDocument>,
}

/// Canonical storage iterator state behind one interop handle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalStorageIterator {
    id: u32,
    rows: Vec<(Vec<u8>, Vec<u8>)>,
    current: Option<usize>,
    prefix_length: usize,
    options_bits: u8,
}

/// Complete bounded comparison artifact for one application execution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalExecutionArtifact {
    environment: ExecutionEnvironmentArtifact,
    vm_state: VmState,
    instructions_executed: u64,
    gas_consumed_pico: i64,
    fee_consumed_pico: i64,
    reference_count: usize,
    fault_message: Option<String>,
    fault_exception: Option<CanonicalStackValue>,
    result_stack: Vec<CanonicalStackValue>,
    invocation_stack: Vec<CanonicalInvocationFrame>,
    invocation_counters: Vec<(UInt160, u32)>,
    storage_changes: Vec<CanonicalStorageChange>,
    storage_reads: Vec<CanonicalStoragePointObservation>,
    storage_ranges: Vec<CanonicalStorageRangeObservation>,
    native_cache: Vec<CanonicalNativeCacheObservation>,
    calls: Vec<CanonicalCallObservation>,
    pending_native_calls: Vec<CanonicalPendingNativeCall>,
    witnesses: Vec<CanonicalWitnessObservation>,
    contexts: Vec<CanonicalContextObservation>,
    fee_charges: Vec<u64>,
    notifications: Vec<CanonicalNotification>,
    logs: Vec<CanonicalLog>,
    diagnostics: Vec<CanonicalDiagnosticObservation>,
    iterators: Vec<CanonicalStorageIterator>,
    stack_graph: CanonicalStackGraph,
}

/// First complete artifact component that differs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionArtifactComponent {
    /// Protocol, block, container, or host context.
    Environment,
    /// VM state.
    VmState,
    /// Instruction count.
    Instructions,
    /// Gas or fee accounting.
    Gas,
    /// Reference-counter total.
    ReferenceCount,
    /// Fault message or exception stack item.
    Fault,
    /// Result stack roots.
    ResultStack,
    /// Active invocation frames and their stacks/slots.
    InvocationStack,
    /// Logical invocation counters.
    InvocationCounters,
    /// Final storage writes/deletes.
    StorageChanges,
    /// Point reads.
    StorageReads,
    /// Range reads.
    StorageRanges,
    /// Native-cache reads and changes.
    NativeCaches,
    /// Completed or pending calls.
    Calls,
    /// Witness results.
    Witnesses,
    /// External context reads.
    Contexts,
    /// Explicit fee-charge sequence.
    FeeCharges,
    /// Notifications.
    Notifications,
    /// Logs.
    Logs,
    /// Diagnostic callbacks.
    Diagnostics,
    /// Storage iterator state.
    Iterators,
    /// Normalized stack object contents or alias topology.
    StackGraph,
}

/// Bounded mismatch identity. Reproducer persistence is added by shadow routing.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("execution artifacts differ at {component:?}{}", self.detail_display())]
pub struct ExecutionArtifactMismatch {
    component: ExecutionArtifactComponent,
    detail: Option<ExecutionArtifactMismatchDetail>,
}

/// Bounded first-divergence evidence for one sequence component: lengths,
/// first differing index, and bounded FNV-1a hashes of the two differing
/// elements' debug representations. No unbounded payloads are retained.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutionArtifactMismatchDetail {
    /// Element count in the ordinary (authoritative) sequence.
    pub ordinary_count: u32,
    /// Element count in the candidate sequence.
    pub candidate_count: u32,
    /// First index whose elements differ, or the shorter length when one
    /// sequence is a strict prefix of the other.
    pub first_diff_index: u32,
    /// Bounded hash of the ordinary element at `first_diff_index` (0 when absent).
    pub ordinary_element_hash: u64,
    /// Bounded hash of the candidate element at `first_diff_index` (0 when absent).
    pub candidate_element_hash: u64,
}

impl ExecutionArtifactMismatch {
    /// Returns the first differing component.
    #[must_use]
    pub const fn component(self) -> ExecutionArtifactComponent {
        self.component
    }

    /// Returns bounded first-divergence evidence when the component is a sequence.
    #[must_use]
    pub const fn detail(self) -> Option<ExecutionArtifactMismatchDetail> {
        self.detail
    }

    fn detail_display(&self) -> String {
        match &self.detail {
            Some(detail) => format!(
                " (ordinary {} vs candidate {} elements, first diff at index {}, hashes {:#018x} vs {:#018x})",
                detail.ordinary_count,
                detail.candidate_count,
                detail.first_diff_index,
                detail.ordinary_element_hash,
                detail.candidate_element_hash,
            ),
            None => String::new(),
        }
    }
}

fn fnv1a_debug_hash<T: std::fmt::Debug>(value: &T) -> u64 {
    let debug = format!("{value:?}");
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in debug.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn compare_sequences<T: PartialEq + std::fmt::Debug>(
    ordinary: &[T],
    candidate: &[T],
) -> Option<ExecutionArtifactMismatchDetail> {
    if ordinary == candidate {
        return None;
    }
    let common = ordinary.len().min(candidate.len());
    let first_diff_index = (0..common)
        .find(|&index| ordinary[index] != candidate[index])
        .unwrap_or(common);
    Some(ExecutionArtifactMismatchDetail {
        ordinary_count: ordinary.len() as u32,
        candidate_count: candidate.len() as u32,
        first_diff_index: first_diff_index as u32,
        ordinary_element_hash: ordinary
            .get(first_diff_index)
            .map(fnv1a_debug_hash)
            .unwrap_or(0),
        candidate_element_hash: candidate
            .get(first_diff_index)
            .map(fnv1a_debug_hash)
            .unwrap_or(0),
    })
}

impl CanonicalExecutionArtifact {
    /// Compares every observable component in a stable order.
    pub fn compare(&self, optimized: &Self) -> Result<(), ExecutionArtifactMismatch> {
        macro_rules! compare {
            ($field:ident, $component:ident) => {
                if self.$field != optimized.$field {
                    return Err(ExecutionArtifactMismatch {
                        component: ExecutionArtifactComponent::$component,
                        detail: None,
                    });
                }
            };
        }
        macro_rules! compare_seq {
            ($field:ident, $component:ident) => {
                if let Some(detail) = compare_sequences(&self.$field, &optimized.$field) {
                    return Err(ExecutionArtifactMismatch {
                        component: ExecutionArtifactComponent::$component,
                        detail: Some(detail),
                    });
                }
            };
        }
        compare!(environment, Environment);
        compare!(vm_state, VmState);
        compare!(instructions_executed, Instructions);
        if self.gas_consumed_pico != optimized.gas_consumed_pico
            || self.fee_consumed_pico != optimized.fee_consumed_pico
        {
            return Err(ExecutionArtifactMismatch {
                component: ExecutionArtifactComponent::Gas,
                detail: None,
            });
        }
        compare!(reference_count, ReferenceCount);
        if self.fault_message != optimized.fault_message
            || self.fault_exception != optimized.fault_exception
        {
            return Err(ExecutionArtifactMismatch {
                component: ExecutionArtifactComponent::Fault,
                detail: None,
            });
        }
        compare_seq!(result_stack, ResultStack);
        compare_seq!(invocation_stack, InvocationStack);
        compare_seq!(invocation_counters, InvocationCounters);
        compare_seq!(storage_changes, StorageChanges);
        compare_seq!(storage_reads, StorageReads);
        compare_seq!(storage_ranges, StorageRanges);
        compare_seq!(native_cache, NativeCaches);
        if self.calls != optimized.calls
            || self.pending_native_calls != optimized.pending_native_calls
        {
            return Err(ExecutionArtifactMismatch {
                component: ExecutionArtifactComponent::Calls,
                detail: None,
            });
        }
        compare_seq!(witnesses, Witnesses);
        compare_seq!(contexts, Contexts);
        compare_seq!(fee_charges, FeeCharges);
        compare_seq!(notifications, Notifications);
        compare_seq!(logs, Logs);
        compare_seq!(diagnostics, Diagnostics);
        compare_seq!(iterators, Iterators);
        compare!(stack_graph, StackGraph);
        Ok(())
    }

    /// Returns the final VM state.
    #[must_use]
    pub const fn vm_state(&self) -> VmState {
        self.vm_state
    }

    /// Returns the exact raw execution fee consumed.
    #[must_use]
    pub const fn fee_consumed_pico(&self) -> i64 {
        self.fee_consumed_pico
    }

    /// Returns the canonical result roots in bottom-to-top stack order.
    #[must_use]
    pub fn result_stack(&self) -> &[CanonicalStackValue] {
        &self.result_stack
    }

    /// Returns the shared normalized stack graph.
    #[must_use]
    pub const fn stack_graph(&self) -> &CanonicalStackGraph {
        &self.stack_graph
    }
}

mod capture;
