//! Commit-pipeline context port for the state-service subsystem.
//!
//! Phase 1 S3 (Batch A): the commit handlers used to downcast `&dyn Any` to
//! `NeoSystem`/`NeoSystemContext` to reach the registered state store and the
//! actor event stream. This port inverted that joint: the handlers consume a
//! `StateCommitContext`, and the runtime glue (neo-system) supplies an adapter
//! over `NeoSystemContext`. Embedders without a runtime context pass `None`
//! and retain the post-commit fallback in `StateRootStore::accept_local_root`.

use super::commit_handlers::StateServiceError;
use super::services::StateStoreService;
use super::ValidatedRootPersisted;
use std::sync::Arc;

/// Runtime services required by the state-root commit pipeline.
pub trait StateCommitContext: Send + Sync {
    /// Resolves the registered state-store service, if any.
    fn state_store(&self) -> Result<Option<Arc<dyn StateStoreService>>, StateServiceError>;

    /// Publishes the validated-root-persisted event on the runtime event stream.
    fn publish_validated_root_persisted(&self, event: ValidatedRootPersisted);
}