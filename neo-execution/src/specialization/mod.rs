//! # Guarded execution specialization
//!
//! Candidate kernels compute fresh `neo_vm::StackItem` values for one
//! invocation. Routing, shadow comparison, and authority remain separate and
//! disabled by default.
//!
//! ## Boundary
//!
//! This module owns candidate control and pure candidate kernels. Ordinary
//! `neo-vm` remains semantic authority; state publication and node policy live
//! outside this module.
//!
//! ## Contents
//!
//! - Bounded candidate routing, latches, and mismatch reproducers.
//! - The exact-version Flamingo pair-key candidate.

mod control;
mod flamingo_factory_pair_key;

pub use control::{
    CandidateControlSnapshot, CandidateRouteConfig, MismatchRecordOutcome, SpecializationControl,
    SpecializationControlConfig, SpecializationControlConfigError, SpecializationControlError,
    SpecializationControlLimits, SpecializationControlSnapshot, SpecializationDisableReason,
    SpecializationMismatchInput, SpecializationMismatchReproducer, SpecializationRouteDecision,
};

pub use flamingo_factory_pair_key::{
    FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_ID, FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_VERSION,
    FLAMINGO_FACTORY_PAIR_KEY_ENTRY, FlamingoPairKeyArtifact, FlamingoPairKeyEligibilityError,
    flamingo_pair_key_candidate, try_flamingo_pair_key,
};
