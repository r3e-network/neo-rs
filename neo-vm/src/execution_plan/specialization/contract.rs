//! Immutable eligibility, dependency, gas, fault, and effect declarations.

use super::super::{ContractResolutionIdentity, ExecutionPlanKey};
use crate::{StackItem, StackItemType};
use neo_primitives::UInt160;
use std::collections::HashSet;
use std::mem::size_of;
use std::sync::Arc;

mod state_and_effects;
mod validation;

pub use state_and_effects::{
    CallContract, ContractTarget, EffectContract, FaultClass, FaultContract,
    FaultEffectDisposition, GasAmount, GasStepContract, HostEffectContract, InstructionCount,
    NativeCacheDependency, NativeCacheDomain, NativeCacheScope, PointStateDependency,
    RangeDirection, RangeDomain, RangeStateDependency, ReadRequirement, StackEffectContract,
    StateDependencyContract, StorageTarget, StorageWriteKind,
};
pub use validation::CandidateContractError;
use validation::{accounted_bytes, validate_limits, validate_parts};

/// Schema version for a [`CandidateContract`].
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct SpecializationContractVersion(u16);

impl SpecializationContractVersion {
    /// Initial exact-identity and explicit-effect contract schema.
    pub const V1: Self = Self(1);

    /// Returns the numeric schema version.
    #[must_use]
    pub const fn value(self) -> u16 {
        self.0
    }
}

/// Stable process-independent identifier for one specialization candidate.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct CandidateId(u32);

impl CandidateId {
    /// Creates an identifier. Zero is reserved and rejected by contract validation.
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Returns the numeric identifier.
    #[must_use]
    pub const fn value(self) -> u32 {
        self.0
    }
}

/// Monotonic implementation and declaration version for one candidate ID.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct CandidateVersion(u16);

impl CandidateVersion {
    /// Creates a candidate version. Zero is reserved and rejected by validation.
    #[must_use]
    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    /// Returns the numeric candidate version.
    #[must_use]
    pub const fn value(self) -> u16 {
        self.0
    }
}

/// Exact versioned identity for one deployed-script specialization.
///
/// [`ExecutionPlanKey`] retains exact script bytes as well as network,
/// protocol, complete hardfork table, trigger, entry byte offset, and deployed
/// contract hash/ID/update/NEF identity. Hash160 is only a lookup hint; key
/// equality still compares the retained bytecode.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CandidateIdentity {
    schema: SpecializationContractVersion,
    candidate_id: CandidateId,
    candidate_version: CandidateVersion,
    execution: ExecutionPlanKey,
}

impl CandidateIdentity {
    /// Creates a v1 candidate identity around an exact execution identity.
    #[must_use]
    pub const fn new(
        candidate_id: CandidateId,
        candidate_version: CandidateVersion,
        execution: ExecutionPlanKey,
    ) -> Self {
        Self {
            schema: SpecializationContractVersion::V1,
            candidate_id,
            candidate_version,
            execution,
        }
    }

    /// Returns the specialization contract schema version.
    #[must_use]
    pub const fn schema(&self) -> SpecializationContractVersion {
        self.schema
    }

    /// Returns the stable candidate identifier.
    #[must_use]
    pub const fn candidate_id(&self) -> CandidateId {
        self.candidate_id
    }

    /// Returns the candidate implementation version.
    #[must_use]
    pub const fn candidate_version(&self) -> CandidateVersion {
        self.candidate_version
    }

    /// Returns the exact execution identity.
    #[must_use]
    pub const fn execution(&self) -> &ExecutionPlanKey {
        &self.execution
    }
}

/// Highest authority that an explicitly enabled candidate may receive.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CandidateAuthority {
    /// The candidate may only run in an isolated differential shadow.
    ShadowOnly,
    /// Correctness gates permit explicit opt-in authoritative routing.
    OptInAuthoritative,
}

/// Value restriction for one concrete NeoVM stack-item type.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum StackItemConstraint {
    /// Any value of the declared concrete stack-item type.
    Any,
    /// Exact bytes for a `ByteString` or `Buffer`.
    ExactBytes(Arc<[u8]>),
    /// Inclusive byte-length bounds for a `ByteString` or `Buffer`.
    ByteLength {
        /// Minimum accepted length.
        min: u32,
        /// Maximum accepted length.
        max: u32,
    },
    /// Inclusive element-count bounds for an array, struct, or map.
    CollectionLength {
        /// Minimum accepted element count.
        min: u32,
        /// Maximum accepted element count.
        max: u32,
    },
    /// Exact Boolean value.
    ExactBoolean(bool),
    /// Inclusive signed range for a representable NeoVM integer.
    SignedIntegerRange {
        /// Minimum accepted value.
        min: i64,
        /// Maximum accepted value.
        max: i64,
    },
}

/// One concrete stack-item shape accepted or produced by a candidate.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct StackItemShape {
    item_type: StackItemType,
    constraint: StackItemConstraint,
}

impl StackItemShape {
    /// Creates and validates a concrete NeoVM stack-item shape.
    ///
    /// # Errors
    ///
    /// Returns an error when the value constraint cannot apply to `item_type`
    /// or when an inclusive range is reversed.
    pub fn new(
        item_type: StackItemType,
        constraint: StackItemConstraint,
    ) -> Result<Self, CandidateContractError> {
        let shape = Self {
            item_type,
            constraint,
        };
        shape.validate()?;
        Ok(shape)
    }

    /// Returns the exact NeoVM type tag.
    #[must_use]
    pub const fn item_type(&self) -> StackItemType {
        self.item_type
    }

    /// Returns the value restriction.
    #[must_use]
    pub const fn constraint(&self) -> &StackItemConstraint {
        &self.constraint
    }

    /// Tests one ordinary `neo-vm` stack item without converting value models.
    #[must_use]
    pub fn matches(&self, item: &StackItem) -> bool {
        if item.stack_item_type() != self.item_type {
            return false;
        }

        match (&self.constraint, item) {
            (StackItemConstraint::Any, _) => true,
            (StackItemConstraint::ExactBytes(expected), StackItem::ByteString(actual)) => {
                expected.as_ref() == actual.as_slice()
            }
            (StackItemConstraint::ExactBytes(expected), StackItem::Buffer(actual)) => {
                actual.with_data(|bytes| bytes == expected.as_ref())
            }
            (StackItemConstraint::ByteLength { min, max }, StackItem::ByteString(bytes)) => {
                length_in_range(bytes.len(), *min, *max)
            }
            (StackItemConstraint::ByteLength { min, max }, StackItem::Buffer(buffer)) => {
                length_in_range(buffer.len(), *min, *max)
            }
            (StackItemConstraint::CollectionLength { min, max }, StackItem::Array(array)) => {
                length_in_range(array.len(), *min, *max)
            }
            (StackItemConstraint::CollectionLength { min, max }, StackItem::Struct(value)) => {
                length_in_range(value.len(), *min, *max)
            }
            (StackItemConstraint::CollectionLength { min, max }, StackItem::Map(map)) => {
                length_in_range(map.len(), *min, *max)
            }
            (StackItemConstraint::ExactBoolean(expected), StackItem::Boolean(actual)) => {
                expected == actual
            }
            (StackItemConstraint::SignedIntegerRange { min, max }, StackItem::Integer(value)) => {
                value
                    .to_i64()
                    .is_some_and(|value| value >= *min && value <= *max)
            }
            _ => false,
        }
    }

    fn validate(&self) -> Result<(), CandidateContractError> {
        let valid = match &self.constraint {
            StackItemConstraint::Any => true,
            StackItemConstraint::ExactBytes(_) => matches!(
                self.item_type,
                StackItemType::ByteString | StackItemType::Buffer
            ),
            StackItemConstraint::ByteLength { min, max } => {
                min <= max
                    && matches!(
                        self.item_type,
                        StackItemType::ByteString | StackItemType::Buffer
                    )
            }
            StackItemConstraint::CollectionLength { min, max } => {
                min <= max
                    && matches!(
                        self.item_type,
                        StackItemType::Array | StackItemType::Struct | StackItemType::Map
                    )
            }
            StackItemConstraint::ExactBoolean(_) => self.item_type == StackItemType::Boolean,
            StackItemConstraint::SignedIntegerRange { min, max } => {
                min <= max && self.item_type == StackItemType::Integer
            }
        };

        if valid {
            Ok(())
        } else {
            Err(CandidateContractError::InvalidStackItemConstraint)
        }
    }

    fn dynamic_bytes(&self) -> usize {
        match &self.constraint {
            StackItemConstraint::ExactBytes(bytes) => bytes.len(),
            _ => 0,
        }
    }
}

fn length_in_range(length: usize, min: u32, max: u32) -> bool {
    u32::try_from(length).is_ok_and(|length| length >= min && length <= max)
}

/// Non-empty alternatives accepted for one invocation value.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct StackItemEligibility {
    accepted: Arc<[StackItemShape]>,
}

impl StackItemEligibility {
    /// Creates a non-empty exact set of accepted stack-item shapes.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty set, an invalid shape, or duplicate shape.
    pub fn new(accepted: impl Into<Arc<[StackItemShape]>>) -> Result<Self, CandidateContractError> {
        let accepted = accepted.into();
        if accepted.is_empty() {
            return Err(CandidateContractError::EmptyStackItemEligibility);
        }
        let mut unique = HashSet::with_capacity(accepted.len());
        for shape in accepted.iter() {
            shape.validate()?;
            if !unique.insert(shape) {
                return Err(CandidateContractError::DuplicateStackItemShape);
            }
        }
        Ok(Self { accepted })
    }

    /// Returns all accepted concrete shapes.
    #[must_use]
    pub fn accepted(&self) -> &[StackItemShape] {
        &self.accepted
    }

    /// Tests an ordinary `neo-vm` stack item against every accepted shape.
    #[must_use]
    pub fn matches(&self, item: &StackItem) -> bool {
        self.accepted.iter().any(|shape| shape.matches(item))
    }

    fn dynamic_bytes(&self) -> usize {
        self.accepted
            .iter()
            .map(StackItemShape::dynamic_bytes)
            .sum::<usize>()
            .saturating_add(size_of::<StackItemShape>().saturating_mul(self.accepted.len()))
    }
}

/// Eligibility declaration for one normalized method argument.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ArgumentContract {
    index: u16,
    value: StackItemEligibility,
}

impl ArgumentContract {
    /// Creates a declaration for a zero-based normalized method argument.
    #[must_use]
    pub const fn new(index: u16, value: StackItemEligibility) -> Self {
        Self { index, value }
    }

    /// Returns the zero-based normalized method-argument index.
    #[must_use]
    pub const fn index(&self) -> u16 {
        self.index
    }

    /// Returns accepted NeoVM value shapes.
    #[must_use]
    pub const fn value(&self) -> &StackItemEligibility {
        &self.value
    }
}

/// VM frame slot whose value is an eligibility or execution dependency.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SlotSource {
    /// Current context argument slot.
    Argument,
    /// Current context local slot.
    Local,
    /// Current context static-field slot.
    Static,
}

/// Declared read of one current-context VM slot.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SlotContract {
    source: SlotSource,
    index: u16,
    value: StackItemEligibility,
}

impl SlotContract {
    /// Creates a declared slot read and accepted-value contract.
    #[must_use]
    pub const fn new(source: SlotSource, index: u16, value: StackItemEligibility) -> Self {
        Self {
            source,
            index,
            value,
        }
    }

    /// Returns the slot domain.
    #[must_use]
    pub const fn source(&self) -> SlotSource {
        self.source
    }

    /// Returns the zero-based slot index.
    #[must_use]
    pub const fn index(&self) -> u16 {
        self.index
    }

    /// Returns accepted NeoVM value shapes.
    #[must_use]
    pub const fn value(&self) -> &StackItemEligibility {
        &self.value
    }
}

/// Execution-context value that a candidate is permitted to observe.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum ContextDependency {
    /// Script container, including transaction or block identity and fields.
    ScriptContainer,
    /// Persisting block identity and fields.
    PersistingBlock,
    /// Remaining gas before or during the candidate.
    GasRemaining,
    /// Whether the current application context bypasses per-opcode fees.
    FeeWhitelist {
        /// Exact fee-bypass state required for eligibility.
        expected: bool,
    },
    /// Exact NeoVM `CALL` clone provenance: a parent frame exists, evaluation
    /// stack and static fields are shared, return count is zero, and local,
    /// argument, and exception slots are uninitialized.
    InternalCallFrame,
    /// Current call flags.
    CallFlags {
        /// Bits that must be present for eligibility.
        required: u32,
        /// Bits that must be absent for eligibility.
        forbidden: u32,
    },
    /// Current contract invocation counter.
    InvocationCounter,
    /// Entry script hash.
    EntryScriptHash,
    /// Calling script hash, optionally constrained to one exact value.
    CallingScriptHash {
        /// Exact required value, or `None` when any value is supported.
        expected: Option<UInt160>,
    },
    /// Executing script hash. The candidate identity still fixes exact bytes.
    ExecutingScriptHash,
    /// Runtime timestamp.
    RuntimeTime,
    /// Runtime random value and counter.
    RuntimeRandom,
    /// Diagnostic-listener presence and diagnostic state.
    Diagnostics,
}

/// Exact normalized invocation eligibility and declared VM/context reads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvocationEligibility {
    arguments: Arc<[ArgumentContract]>,
    slots: Arc<[SlotContract]>,
    context: Arc<[ContextDependency]>,
}

impl InvocationEligibility {
    /// Creates immutable eligibility declarations.
    #[must_use]
    pub fn new(
        arguments: impl Into<Arc<[ArgumentContract]>>,
        slots: impl Into<Arc<[SlotContract]>>,
        context: impl Into<Arc<[ContextDependency]>>,
    ) -> Self {
        Self {
            arguments: arguments.into(),
            slots: slots.into(),
            context: context.into(),
        }
    }

    /// Returns normalized argument declarations in required index order.
    #[must_use]
    pub fn arguments(&self) -> &[ArgumentContract] {
        &self.arguments
    }

    /// Returns declared VM slot reads.
    #[must_use]
    pub fn slots(&self) -> &[SlotContract] {
        &self.slots
    }

    /// Returns declared execution-context reads.
    #[must_use]
    pub fn context(&self) -> &[ContextDependency] {
        &self.context
    }

    /// Checks exact arity and every normalized argument using `neo-vm` values.
    #[must_use]
    pub fn matches_arguments(&self, arguments: &[StackItem]) -> bool {
        arguments.len() == self.arguments.len()
            && self
                .arguments
                .iter()
                .zip(arguments)
                .all(|(contract, argument)| contract.value.matches(argument))
    }
}

/// One segment in a bounded byte expression used for state and effect keys.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum ByteExpressionSegment {
    /// Exact immutable bytes.
    Literal(Arc<[u8]>),
    /// NeoVM byte conversion of one normalized method argument.
    Argument(u16),
    /// NeoVM byte conversion of one declared current-context slot.
    Slot {
        /// Slot domain.
        source: SlotSource,
        /// Zero-based slot index.
        index: u16,
    },
    /// One declared context script hash encoded as 20 bytes.
    ScriptHash(ContextScriptHash),
}

/// Script-hash context available to a [`ByteExpressionSegment`].
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ContextScriptHash {
    /// Entry script hash.
    Entry,
    /// Calling script hash.
    Calling,
    /// Executing script hash.
    Executing,
}

/// Flat, bounded concatenation recipe for a storage key or effect target.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ByteExpression {
    segments: Arc<[ByteExpressionSegment]>,
}

impl ByteExpression {
    /// Creates a non-empty sequence of byte-expression segments.
    ///
    /// A single empty literal explicitly represents an empty byte string.
    ///
    /// # Errors
    ///
    /// Returns an error when no segment is supplied.
    pub fn new(
        segments: impl Into<Arc<[ByteExpressionSegment]>>,
    ) -> Result<Self, CandidateContractError> {
        let segments = segments.into();
        if segments.is_empty() {
            return Err(CandidateContractError::EmptyByteExpression);
        }
        Ok(Self { segments })
    }

    /// Creates one exact literal expression.
    #[must_use]
    pub fn literal(bytes: impl Into<Arc<[u8]>>) -> Self {
        Self {
            segments: Arc::from([ByteExpressionSegment::Literal(bytes.into())]),
        }
    }

    /// Returns the ordered expression segments.
    #[must_use]
    pub fn segments(&self) -> &[ByteExpressionSegment] {
        &self.segments
    }

    fn literal_bytes(&self) -> usize {
        self.segments
            .iter()
            .map(|segment| match segment {
                ByteExpressionSegment::Literal(bytes) => bytes.len(),
                _ => 0,
            })
            .sum()
    }

    fn accounted_bytes(&self) -> usize {
        size_of::<ByteExpressionSegment>()
            .saturating_mul(self.segments.len())
            .saturating_add(self.literal_bytes())
    }
}

/// Hard per-candidate declaration limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CandidateContractLimits {
    /// Maximum normalized argument count.
    pub max_arguments: usize,
    /// Maximum accepted shapes for one argument, slot, or result.
    pub max_shapes_per_value: usize,
    /// Maximum declared current-context slot reads.
    pub max_slots: usize,
    /// Maximum declared execution-context reads.
    pub max_context_dependencies: usize,
    /// Maximum point storage reads.
    pub max_point_dependencies: usize,
    /// Maximum range or prefix reads.
    pub max_range_dependencies: usize,
    /// Maximum native-cache reads.
    pub max_native_dependencies: usize,
    /// Maximum ordered gas steps.
    pub max_gas_steps: usize,
    /// Maximum possible faults.
    pub max_faults: usize,
    /// Maximum possible host-visible effects.
    pub max_host_effects: usize,
    /// Maximum segments in one byte expression.
    pub max_expression_segments: usize,
    /// Maximum total literal bytes in one byte expression.
    pub max_expression_literal_bytes: usize,
}

impl CandidateContractLimits {
    /// Conservative bounds for a small, manually audited candidate.
    pub const DEFAULT: Self = Self {
        max_arguments: 32,
        max_shapes_per_value: 8,
        max_slots: 32,
        max_context_dependencies: 32,
        max_point_dependencies: 64,
        max_range_dependencies: 16,
        max_native_dependencies: 32,
        max_gas_steps: 128,
        max_faults: 32,
        max_host_effects: 64,
        max_expression_segments: 16,
        max_expression_literal_bytes: 4 * 1024,
    };
}

impl Default for CandidateContractLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Inputs used to construct one validated immutable candidate contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateContractParts {
    /// Exact versioned candidate and execution identity.
    pub identity: CandidateIdentity,
    /// Maximum authority allowed after external promotion gates.
    pub authority: CandidateAuthority,
    /// Argument, VM-slot, and context eligibility declarations.
    pub eligibility: InvocationEligibility,
    /// Point, range, and native-cache dependencies.
    pub state: StateDependencyContract,
    /// Exact fixed or input-dependent VM instruction count.
    pub instruction_count: InstructionCount,
    /// Ordered exact gas charges.
    pub gas_steps: Arc<[GasStepContract]>,
    /// Complete possible fault classes.
    pub faults: Arc<[FaultContract]>,
    /// Complete stack and host-effect surface.
    pub effects: EffectContract,
}

/// Validated immutable specialization declaration.
///
/// This type never contains a final stack, gas result, state delta,
/// notification, witness result, or other execution output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateContract {
    parts: CandidateContractParts,
    accounted_bytes: usize,
}

impl CandidateContract {
    /// Validates and constructs an immutable candidate contract.
    ///
    /// # Errors
    ///
    /// Returns an error for incomplete identity, unbounded declarations,
    /// duplicate IDs, invalid references, or an internally inconsistent effect
    /// contract.
    pub fn try_new(
        parts: CandidateContractParts,
        limits: CandidateContractLimits,
    ) -> Result<Self, CandidateContractError> {
        validate_limits(limits)?;
        validate_parts(&parts, limits)?;
        let accounted_bytes = accounted_bytes(&parts);
        Ok(Self {
            parts,
            accounted_bytes,
        })
    }

    /// Returns the exact versioned identity.
    #[must_use]
    pub const fn identity(&self) -> &CandidateIdentity {
        &self.parts.identity
    }

    /// Returns the maximum externally promoted authority.
    #[must_use]
    pub const fn authority(&self) -> CandidateAuthority {
        self.parts.authority
    }

    /// Returns exact invocation eligibility declarations.
    #[must_use]
    pub const fn eligibility(&self) -> &InvocationEligibility {
        &self.parts.eligibility
    }

    /// Returns exact point, range, and native-cache dependencies.
    #[must_use]
    pub const fn state(&self) -> &StateDependencyContract {
        &self.parts.state
    }

    /// Returns the exact instruction-count declaration.
    #[must_use]
    pub const fn instruction_count(&self) -> InstructionCount {
        self.parts.instruction_count
    }

    /// Returns ordered gas steps.
    #[must_use]
    pub fn gas_steps(&self) -> &[GasStepContract] {
        &self.parts.gas_steps
    }

    /// Returns every possible declared fault.
    #[must_use]
    pub fn faults(&self) -> &[FaultContract] {
        &self.parts.faults
    }

    /// Returns complete stack and host-effect declarations.
    #[must_use]
    pub const fn effects(&self) -> &EffectContract {
        &self.parts.effects
    }

    /// Returns deterministic contract payload bytes used for registry bounds.
    #[must_use]
    pub const fn accounted_bytes(&self) -> usize {
        self.accounted_bytes
    }

    pub(super) fn validate_against(
        &self,
        limits: CandidateContractLimits,
    ) -> Result<(), CandidateContractError> {
        validate_limits(limits)?;
        validate_parts(&self.parts, limits)
    }
}
