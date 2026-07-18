//! # NeoVM execution plans
//!
//! Immutable, versioned execution plans for NeoVM bytecode.
//!
//! ## Boundary
//!
//! Plans accelerate decoding and dispatch while preserving ordinary NeoVM
//! stack items, gas, faults, diagnostics, and host interactions. This module
//! neither executes contracts outside NeoVM nor caches stateful results.
//!
//! ## Contents
//!
//! - Bounded concurrent plan cache.
//! - Exact protocol, hardfork, script, and contract identities.
//! - Verified basic blocks and guarded specialization contracts.

mod cache;
mod identity;
mod plan;
mod specialization;

pub use cache::{
    ExecutionPlanCache, ExecutionPlanCacheError, ExecutionPlanCacheLimits,
    ExecutionPlanCacheSnapshot,
};
pub use identity::{
    ContractResolutionIdentity, ExecutionPlanKey, ExecutionPlanKeyVersion, HardforkPlanState,
    HardforkTableIdentity, ProtocolIdentity, ProtocolVersion,
};
pub use plan::{
    BasicBlock, ExecutionPlan, ExecutionPlanBuildError, ExecutionPlanLimits, ExecutionPlanRoute,
    PlannedControlFlow, PlannedInstruction,
};
pub use specialization::{
    ArgumentContract, ByteExpression, ByteExpressionSegment, CallContract, CandidateAuthority,
    CandidateContract, CandidateContractError, CandidateContractLimits, CandidateContractParts,
    CandidateId, CandidateIdentity, CandidateVersion, ContextDependency, ContextScriptHash,
    ContractTarget, EffectContract, FaultClass, FaultContract, FaultEffectDisposition, GasAmount,
    GasStepContract, HostEffectContract, InstructionCount, InvocationEligibility,
    NativeCacheDependency, NativeCacheDomain, NativeCacheScope, PointStateDependency,
    RangeDirection, RangeDomain, RangeStateDependency, ReadRequirement, RegistryBuildError,
    RegistrySnapshot, SlotContract, SlotSource, SpecializationContractVersion, SpecializationMode,
    SpecializationRegistry, SpecializationRegistryLimits, SpecializationSelection,
    StackEffectContract, StackItemConstraint, StackItemEligibility, StackItemShape,
    StateDependencyContract, StorageTarget, StorageWriteKind,
};

#[cfg(test)]
#[path = "../tests/execution_plan/differential.rs"]
mod differential_tests;
