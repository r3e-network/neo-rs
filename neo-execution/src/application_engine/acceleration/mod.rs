//! # neo-execution::application_engine::acceleration
//!
//! Rebuildable execution plans, guarded specializations, and their
//! ordinary-authoritative shadow comparison.
//!
//! ## Boundary
//!
//! This module may accelerate script preparation or isolated candidate
//! execution, but NeoVM remains the semantic authority. It must not cache
//! execution results, publish candidate state, or bypass full artifact parity.
//!
//! ## Contents
//!
//! - `execution_plans`: bounded immutable NeoVM plan reuse.
//! - `specializations`: exact-identity guarded script short paths.
//! - `shadow`: isolated twin execution and canonical parity enforcement.

mod execution_plans;
mod shadow;
mod specializations;

pub use execution_plans::{ApplicationExecutionPlanCache, ApplicationExecutionPlanConfig};
pub use shadow::{
    FlamingoShadowOutcome, FlamingoShadowRunError, PreparedShadowEngine, ShadowInfrastructureStage,
    ShadowObservationBinding, ShadowReplayStatus, ShadowStrictReplayFailure,
    ShadowStrictReplayFailureKind, ShadowTwinBranch, ShadowTwinResources, run_flamingo_shadow_pair,
};
