//! State dependencies, accounting, faults, and effects for candidates.

use super::*;

/// Contract whose storage is addressed by a dependency or effect.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum StorageTarget {
    /// The exact deployed contract in the candidate identity.
    ExecutingContract,
    /// Another exact deployed contract version.
    ExactContract(ContractResolutionIdentity),
}

/// Required point-read presence semantics.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ReadRequirement {
    /// The key must be present for specialized eligibility.
    Present,
    /// The key must be absent for specialized eligibility.
    Absent,
    /// Presence and absence are both supported and remain observed dependencies.
    PresentOrAbsent,
}

/// Exact declared storage point read.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PointStateDependency {
    /// Candidate-local dependency identifier used by the audited host facade.
    pub id: u16,
    /// Exact contract selector.
    pub target: StorageTarget,
    /// Bounded key derivation.
    pub key: ByteExpression,
    /// Required presence semantics.
    pub requirement: ReadRequirement,
}

/// Ordered storage range domain.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum RangeDomain {
    /// Every key beginning with an exact derived prefix.
    Prefix(ByteExpression),
    /// Lexicographic half-open interval `[start, end)`.
    HalfOpen {
        /// Inclusive start expression.
        start: ByteExpression,
        /// Exclusive end expression.
        end: ByteExpression,
    },
}

/// Required range traversal direction.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RangeDirection {
    /// Ascending lexicographic order.
    Forward,
    /// Descending lexicographic order.
    Reverse,
}

/// Exact declared storage range read, including phantom-sensitive domain.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct RangeStateDependency {
    /// Candidate-local dependency identifier used by the audited host facade.
    pub id: u16,
    /// Exact contract selector.
    pub target: StorageTarget,
    /// Exact range or prefix domain.
    pub domain: RangeDomain,
    /// Required deterministic traversal order.
    pub direction: RangeDirection,
    /// Maximum number of rows the candidate supports before fallback.
    pub max_items: u32,
}

/// Exact native-cache namespace independent of Rust implementation type names.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct NativeCacheDomain {
    /// Native contract hash.
    pub contract_hash: UInt160,
    /// Persisted native contract ID.
    pub contract_id: i32,
    /// Version of the native cache schema or native implementation.
    pub native_version: u32,
    /// Stable candidate-declared cache partition identifier.
    pub partition: u16,
}

/// Native-cache observation scope.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum NativeCacheScope {
    /// One exact derived cache entry.
    Entry(ByteExpression),
    /// The entire declared cache domain, conservatively versioned as one unit.
    WholeDomain,
}

/// Exact declared native-cache read.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct NativeCacheDependency {
    /// Candidate-local dependency identifier used by the audited host facade.
    pub id: u16,
    /// Exact native cache domain.
    pub domain: NativeCacheDomain,
    /// Exact entry or conservative whole-domain scope.
    pub scope: NativeCacheScope,
}

/// Complete declared state-read surface of one candidate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StateDependencyContract {
    pub(super) point_reads: Arc<[PointStateDependency]>,
    pub(super) range_reads: Arc<[RangeStateDependency]>,
    pub(super) native_reads: Arc<[NativeCacheDependency]>,
}

impl StateDependencyContract {
    /// Creates immutable point, range, and native-cache read declarations.
    #[must_use]
    pub fn new(
        point_reads: impl Into<Arc<[PointStateDependency]>>,
        range_reads: impl Into<Arc<[RangeStateDependency]>>,
        native_reads: impl Into<Arc<[NativeCacheDependency]>>,
    ) -> Self {
        Self {
            point_reads: point_reads.into(),
            range_reads: range_reads.into(),
            native_reads: native_reads.into(),
        }
    }

    /// Returns declared storage point reads.
    #[must_use]
    pub fn point_reads(&self) -> &[PointStateDependency] {
        &self.point_reads
    }

    /// Returns declared phantom-sensitive range reads.
    #[must_use]
    pub fn range_reads(&self) -> &[RangeStateDependency] {
        &self.range_reads
    }

    /// Returns declared native-cache reads.
    #[must_use]
    pub fn native_reads(&self) -> &[NativeCacheDependency] {
        &self.native_reads
    }
}

/// Declarative gas amount for one ordered charge step.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum GasAmount {
    /// One fixed fee in NeoVM gas units.
    Fixed(u64),
    /// One of two exact fees selected by a candidate-local deterministic decision.
    Decision {
        /// Stable candidate-local decision identifier.
        decision: u16,
        /// Fee when the decision is true.
        when_true: u64,
        /// Fee when the decision is false.
        when_false: u64,
    },
    /// Exact base plus per-byte fee for one normalized argument.
    ArgumentBytes {
        /// Zero-based normalized argument index.
        argument: u16,
        /// Fixed base fee.
        base: u64,
        /// Fee per NeoVM-converted byte.
        per_byte: u64,
    },
}

/// Exact VM instruction count produced by one specialized invocation.
///
/// A candidate resolves this declaration before mutating the VM. The executor
/// can then prove that the remaining instruction budget covers the complete
/// path and increment `instructions_executed` exactly as the ordinary
/// interpreter would.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum InstructionCount {
    /// One fixed instruction count.
    Fixed(u64),
    /// One of two exact counts selected by a candidate-local deterministic decision.
    Decision {
        /// Stable candidate-local decision identifier.
        decision: u16,
        /// Instruction count when the decision is true.
        when_true: u64,
        /// Instruction count when the decision is false.
        when_false: u64,
    },
    /// Exact base plus a per-byte count for one normalized argument.
    ArgumentBytes {
        /// Zero-based normalized argument index.
        argument: u16,
        /// Fixed instruction count.
        base: u64,
        /// Additional instructions per NeoVM-converted byte.
        per_byte: u64,
    },
}

/// One ordered gas charge and its exact exhaustion fault.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct GasStepContract {
    /// Candidate-local ordered gas-step identifier.
    pub id: u16,
    /// Exact declarative charge.
    pub amount: GasAmount,
    /// Declared fault identifier when insufficient gas remains.
    pub exhaustion_fault: u16,
}

/// Stable fault class whose exact runtime artifact remains oracle-compared.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FaultClass {
    /// Insufficient gas at a declared gas step.
    OutOfGas,
    /// NeoVM stack underflow or overflow.
    Stack,
    /// Unsupported or invalid stack-item type/value.
    InvalidType,
    /// NeoVM arithmetic or invalid-operation fault.
    InvalidOperation,
    /// Explicit assertion or throw.
    Assertion,
    /// Contract or host failure surfaced as a VM fault.
    Contract,
}

/// One possible specialized fault.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FaultContract {
    /// Candidate-local fault identifier.
    pub id: u16,
    /// Stable fault class. Shadow comparison still checks the complete fault.
    pub class: FaultClass,
    /// Visibility policy for effects already produced before this fault.
    pub effects: FaultEffectDisposition,
}

/// Candidate effect visibility when a declared fault occurs.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FaultEffectDisposition {
    /// Discard every candidate-produced state and externally visible effect.
    Discard,
    /// Preserve effects exactly as the ordinary host does for this fault point.
    Preserve,
}

/// Exact contract call permitted as a candidate effect.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CallContract {
    /// Exact target contract version.
    pub target: ContractTarget,
    /// Entry byte offset within the exact target script.
    pub entry_ip: u32,
    /// Exact call flags supplied to the child context.
    pub call_flags: u32,
    /// Number of normalized call arguments.
    pub argument_count: u16,
    /// Number of child results consumed by the candidate.
    pub result_count: u16,
}

/// Exact deployed contract target for calls and externally visible effects.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ContractTarget {
    /// The exact deployed contract in the candidate identity.
    ExecutingContract,
    /// Another exact deployed contract version.
    Exact(ContractResolutionIdentity),
}

/// Storage mutation kind permitted by an effect declaration.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum StorageWriteKind {
    /// Insert or replace one value.
    Put,
    /// Delete one value.
    Delete,
    /// The candidate may deterministically choose put or delete.
    PutOrDelete,
}

/// One possible host-visible access or effect.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum HostEffectContract {
    /// Point storage mutation.
    StorageWrite {
        /// Exact target contract.
        target: StorageTarget,
        /// Bounded key derivation.
        key: ByteExpression,
        /// Permitted mutation kind.
        kind: StorageWriteKind,
        /// Maximum value bytes for a put; zero for a delete-only effect.
        max_value_bytes: u32,
    },
    /// Native-cache mutation.
    NativeCacheWrite {
        /// Exact native cache domain.
        domain: NativeCacheDomain,
        /// Exact entry or conservative whole-domain scope.
        scope: NativeCacheScope,
    },
    /// Child contract call.
    ContractCall(CallContract),
    /// Runtime notification.
    Notification {
        /// Exact emitter contract.
        emitter: ContractTarget,
        /// Exact UTF-8 event name bytes.
        event_name: Arc<[u8]>,
        /// Maximum event-state item count before fallback.
        max_state_items: u16,
    },
    /// Runtime log entry.
    Log {
        /// Exact emitter contract.
        emitter: ContractTarget,
        /// Maximum UTF-8 message bytes before fallback.
        max_message_bytes: u32,
    },
    /// Witness check whose result is observable by the script.
    WitnessCheck {
        /// Exact or derived 20-byte account expression.
        account: ByteExpression,
    },
    /// Mutation of a current-context VM slot.
    SlotWrite {
        /// Slot domain.
        source: SlotSource,
        /// Zero-based slot index.
        index: u16,
        /// Possible freshly computed NeoVM value shapes.
        value: StackItemEligibility,
    },
}

/// Exact evaluation-stack shape change produced by a candidate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StackEffectContract {
    pub(super) consumed_arguments: u16,
    pub(super) peak_reference_count_delta: u32,
    pub(super) results: Arc<[StackItemEligibility]>,
}

impl StackEffectContract {
    /// Creates a stack-shape and transient-reference declaration.
    ///
    /// `peak_reference_count_delta` is the exact largest increase over the
    /// recursive NeoVM reference count immediately before the ordinary first
    /// instruction. A specialized runner must fall back before effects when
    /// that peak would exceed `MaxStackSize`.
    #[must_use]
    pub fn new(
        consumed_arguments: u16,
        peak_reference_count_delta: u32,
        results: impl Into<Arc<[StackItemEligibility]>>,
    ) -> Self {
        Self {
            consumed_arguments,
            peak_reference_count_delta,
            results: results.into(),
        }
    }

    /// Returns the number of normalized arguments consumed.
    #[must_use]
    pub const fn consumed_arguments(&self) -> u16 {
        self.consumed_arguments
    }

    /// Exact transient recursive-reference increase of the ordinary path.
    #[must_use]
    pub const fn peak_reference_count_delta(&self) -> u32 {
        self.peak_reference_count_delta
    }

    /// Returns possible shape sets for freshly computed results, in push order.
    #[must_use]
    pub fn results(&self) -> &[StackItemEligibility] {
        &self.results
    }
}

/// Complete declared stack and host effect surface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectContract {
    pub(super) stack: StackEffectContract,
    pub(super) host: Arc<[HostEffectContract]>,
}

impl EffectContract {
    /// Creates immutable stack and host-effect declarations.
    #[must_use]
    pub fn new(stack: StackEffectContract, host: impl Into<Arc<[HostEffectContract]>>) -> Self {
        Self {
            stack,
            host: host.into(),
        }
    }

    /// Returns the exact stack-shape effect.
    #[must_use]
    pub const fn stack(&self) -> &StackEffectContract {
        &self.stack
    }

    /// Returns every possible declared host-visible access or effect.
    #[must_use]
    pub fn host(&self) -> &[HostEffectContract] {
        &self.host
    }
}
