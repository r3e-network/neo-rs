//! # Specialization contracts
//!
//! The types in this module describe eligibility and observable behavior. They
//! deliberately contain no executor result, stack snapshot, or state delta.
//! A specialized executor must compute fresh [`crate::StackItem`] values and
//! effects for every invocation, through the ordinary execution host.
//!
//! ## Boundary
//!
//! This module defines immutable candidate contracts and registry mechanics.
//! Candidate execution and authority decisions remain outside the registry.
//!
//! ## Contents
//!
//! - Eligibility, context, state, gas, fault, and effect contracts.
//! - Bounded registry construction and exact-version selection.

mod contract;
mod registry;

pub use contract::{
    ArgumentContract, ByteExpression, ByteExpressionSegment, CallContract, CandidateAuthority,
    CandidateContract, CandidateContractError, CandidateContractLimits, CandidateContractParts,
    CandidateId, CandidateIdentity, CandidateVersion, ContextDependency, ContextScriptHash,
    ContractTarget, EffectContract, FaultClass, FaultContract, FaultEffectDisposition, GasAmount,
    GasStepContract, HostEffectContract, InstructionCount, InvocationEligibility,
    NativeCacheDependency, NativeCacheDomain, NativeCacheScope, PointStateDependency,
    RangeDirection, RangeDomain, RangeStateDependency, ReadRequirement, SlotContract, SlotSource,
    SpecializationContractVersion, StackEffectContract, StackItemConstraint, StackItemEligibility,
    StackItemShape, StateDependencyContract, StorageTarget, StorageWriteKind,
};
pub use registry::{
    RegistryBuildError, RegistrySnapshot, SpecializationMode, SpecializationRegistry,
    SpecializationRegistryLimits, SpecializationSelection,
};

#[cfg(test)]
#[path = "../../tests/execution_plan/specialization.rs"]
mod tests;
