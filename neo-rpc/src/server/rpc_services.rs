//! Typed optional-service handles exposed through the RPC server.
//!
//! RPC plugins are composed once at node startup and read on every request.
//! Keeping each supported service in a named field makes the available
//! surface explicit, preserves each storage backing in the type system, and
//! avoids type-erased lookup and request-path locking.

use std::sync::Arc;

use neo_indexer::IndexerService;
use neo_state_service::StateStore;
use neo_storage::persistence::Store;
use neo_storage::persistence::providers::RuntimeStore;

use crate::application_logs::ApplicationLogsService;
use crate::plugins::tokens_tracker::TokensTrackerService;

/// Optional read-side services available to RPC handlers.
///
/// `S` is the concrete backing used by every storage-backed service. A node
/// cannot accidentally register `StateStore<A>` and request `StateStore<B>`:
/// such a composition does not compile.
pub struct RpcServices<S = RuntimeStore>
where
    S: Store,
{
    state_store: Option<Arc<StateStore<S>>>,
    indexer: Option<Arc<IndexerService>>,
    application_logs: Option<Arc<ApplicationLogsService<S>>>,
    tokens_tracker: Option<Arc<TokensTrackerService<S>>>,
}

impl<S> RpcServices<S>
where
    S: Store,
{
    /// Creates an empty service bundle.
    pub const fn new() -> Self {
        Self {
            state_store: None,
            indexer: None,
            application_logs: None,
            tokens_tracker: None,
        }
    }

    /// Installs the state-service store.
    pub fn with_state_store(mut self, service: Arc<StateStore<S>>) -> Self {
        self.state_store = Some(service);
        self
    }

    /// Installs the indexer service.
    pub fn with_indexer(mut self, service: Arc<IndexerService>) -> Self {
        self.indexer = Some(service);
        self
    }

    /// Installs the application-log service.
    pub fn with_application_logs(mut self, service: Arc<ApplicationLogsService<S>>) -> Self {
        self.application_logs = Some(service);
        self
    }

    /// Installs the token-tracker service.
    pub fn with_tokens_tracker(mut self, service: Arc<TokensTrackerService<S>>) -> Self {
        self.tokens_tracker = Some(service);
        self
    }

    /// Returns the state-service store, when enabled.
    pub fn state_store(&self) -> Option<Arc<StateStore<S>>> {
        self.state_store.as_ref().map(Arc::clone)
    }

    /// Returns the indexer service, when enabled.
    pub fn indexer(&self) -> Option<Arc<IndexerService>> {
        self.indexer.as_ref().map(Arc::clone)
    }

    /// Returns the application-log service, when enabled.
    pub fn application_logs(&self) -> Option<Arc<ApplicationLogsService<S>>> {
        self.application_logs.as_ref().map(Arc::clone)
    }

    /// Returns the token-tracker service, when enabled.
    pub fn tokens_tracker(&self) -> Option<Arc<TokensTrackerService<S>>> {
        self.tokens_tracker.as_ref().map(Arc::clone)
    }
}

impl<S> Clone for RpcServices<S>
where
    S: Store,
{
    fn clone(&self) -> Self {
        Self {
            state_store: self.state_store.as_ref().map(Arc::clone),
            indexer: self.indexer.as_ref().map(Arc::clone),
            application_logs: self.application_logs.as_ref().map(Arc::clone),
            tokens_tracker: self.tokens_tracker.as_ref().map(Arc::clone),
        }
    }
}

impl<S> Default for RpcServices<S>
where
    S: Store,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<S> std::fmt::Debug for RpcServices<S>
where
    S: Store,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RpcServices")
            .field("state_store", &self.state_store.is_some())
            .field("indexer", &self.indexer.is_some())
            .field("application_logs", &self.application_logs.is_some())
            .field("tokens_tracker", &self.tokens_tracker.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_storage::persistence::providers::MemoryStore;

    #[test]
    fn named_slots_preserve_concrete_backing() {
        let state_store = Arc::new(StateStore::<MemoryStore>::new());
        let services = RpcServices::new().with_state_store(Arc::clone(&state_store));

        assert!(Arc::ptr_eq(
            &services.state_store().expect("state store installed"),
            &state_store
        ));
        assert!(services.indexer().is_none());
    }
}
