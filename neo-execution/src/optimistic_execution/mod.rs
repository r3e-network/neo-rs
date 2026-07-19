//! # Optimistic execution foundations
//!
//! This module owns isolation, prefix identity, opt-in dependency capture,
//! deterministic revalidation, and the foundation-only ordered application
//! boundary. Scheduling remains a separate gate.
//!
//! ## Boundary
//!
//! Execution isolation stays in `neo-execution` and reuses the canonical
//! `DataCache`, NeoVM values, and application host. It may merge a validated
//! transaction delta into a caller-owned cache, but does not publish ledger
//! visibility or choose block-import scheduling policy.
//!
//! ## Contents
//!
//! - Pinned block-prefix identity.
//! - Detached per-transaction overlays.
//! - Fail-closed overlay construction errors.
//! - Bounded present/absent pinned-prefix dependency capture.
//! - Caller-owned, ordered point-read revalidation.
//! - Exactly-once HALT storage application and FAULT storage discard.

// Ordered publication stays crate-internal until the scheduler owns an
// exclusive canonical-prefix lane and differential promotion gates pass.
#[allow(dead_code)]
pub(crate) mod application;
// Capture construction stays crate-internal until that scheduler exists. Keep
// the closed prototype compiled and tested without advertising reachable
// production entry points prematurely.
#[allow(dead_code)]
mod artifact;
#[allow(dead_code)]
mod dependencies;
#[allow(dead_code)]
mod host_dependencies;
#[allow(dead_code)]
mod snapshot;
mod validation;

pub use artifact::{
    OptimisticObservationBinding, SpeculativeArtifactCaptureError, SpeculativeEntryScriptIdentity,
    SpeculativeExecutionArtifact, SpeculativeExecutionIdentity, SpeculativeIdentityComponent,
    SpeculativeStorageEffect,
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
