//! # neo-execution::execution_artifact
//!
//! Bounded canonical execution artifacts for differential shadow execution.
//!
//! Artifacts are comparison-only snapshots. They never cross back into the
//! production execution path and never replace NeoVM stack items or execution
//! layer state. Runtime object identities are renumbered by first traversal so
//! two isolated executions can compare equal while aliasing and cycles remain
//! observable.
//!
//! ## Boundary
//!
//! This module captures bounded, deterministic observations after execution.
//! It does not reconstruct runtime values or authorize candidate state changes.
//!
//! ## Contents
//!
//! - Artifact resource bounds and capture errors.
//! - Canonical execution, storage, context, and stack observations.
//! - Deterministic NeoVM object-graph encoding for comparison.

mod bounds;
mod model;
mod observation;
mod stack;

pub use bounds::*;
pub use model::*;
pub use observation::*;
pub use stack::*;

// Internal views consumed by the opt-in optimistic dependency layer. They are
// bounded journal records, not runtime VM values or public artifact handoff
// types.
pub(crate) use observation::ContextObservationSnapshotValue;

#[cfg(test)]
#[path = "../tests/execution_artifact/mod.rs"]
mod tests;
