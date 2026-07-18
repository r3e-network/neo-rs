//! # Optimistic execution foundations
//!
//! This module owns isolation, prefix identity, and opt-in point dependency
//! capture plus deterministic point revalidation. Ordered artifact application
//! and scheduling are separate gates.
//!
//! ## Boundary
//!
//! Execution isolation stays in `neo-execution` and reuses the canonical
//! `DataCache`, NeoVM values, and application host. It does not publish ledger
//! state or choose block-import scheduling policy.
//!
//! ## Contents
//!
//! - Pinned block-prefix identity.
//! - Detached per-transaction overlays.
//! - Fail-closed overlay construction errors.
//! - Bounded present/absent pinned-prefix dependency capture.
//! - Caller-owned, ordered point-read revalidation.

mod artifact;
mod dependencies;
mod host_dependencies;
mod snapshot;
mod validation;

pub use artifact::{
    OptimisticObservationBinding, SpeculativeArtifactCaptureError, SpeculativeExecutionArtifact,
    SpeculativeExecutionIdentity, SpeculativeIdentityComponent, SpeculativeStorageEffect,
};
pub use dependencies::{
    DEFAULT_MAX_POINT_READ_DEPENDENCIES, DEFAULT_MAX_POINT_READ_DEPENDENCY_BYTES,
    DependencyCaptureError, DependencyCaptureLimits, PointReadDependency, TransactionDependencies,
    TransactionDependencyCapture,
};
pub use host_dependencies::{
    HostDependencyCaptureError, HostDependencyValidation, NativeCacheConflictKind,
    NativeCacheDependency, NativeCacheEffect, NativeCacheLocation, OptimisticContextDependency,
    OptimisticContextValue, OptimisticHostDependencies,
};
pub use snapshot::{
    BlockPrefixIdentity, IsolatedTransactionOverlay, OptimisticOverlayError, PinnedBlockPrefix,
};
pub use validation::{PointReadConflict, PointReadConflictKind, PointReadValidation};
