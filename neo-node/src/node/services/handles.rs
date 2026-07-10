//! Typed handles for optional services owned by the node application.
//!
//! Core composition stays in `neo-system::Node`; application services that
//! depend on RPC, indexing, or daemon policy stay here and are passed
//! explicitly to the consumers that need them.

use std::sync::Arc;

use neo_storage::persistence::Store;
use neo_storage::persistence::providers::RuntimeStore;

use super::super::remote_ledger::RemoteLedgerStatus;

/// Optional daemon services with one concrete storage backing.
pub(in crate::node) struct NodeServiceHandles<S = RuntimeStore>
where
    S: Store,
{
    state_store: Option<Arc<neo_state_service::StateStore<S>>>,
    state_commit_handlers:
        Option<Arc<neo_state_service::commit_handlers::StateServiceCommitHandlers<S>>>,
    indexer: Option<Arc<neo_indexer::IndexerService>>,
    application_logs: Option<Arc<neo_rpc::application_logs::ApplicationLogsService<S>>>,
    tokens_tracker: Option<Arc<neo_rpc::plugins::tokens_tracker::TokensTrackerService<S>>>,
    remote_ledger: Option<Arc<RemoteLedgerStatus>>,
}

impl<S> NodeServiceHandles<S>
where
    S: Store,
{
    /// Creates the handle bundle from services built during composition.
    pub(in crate::node) fn new(
        state_store: Option<Arc<neo_state_service::StateStore<S>>>,
        state_commit_handlers: Option<
            Arc<neo_state_service::commit_handlers::StateServiceCommitHandlers<S>>,
        >,
        indexer: Option<Arc<neo_indexer::IndexerService>>,
        application_logs: Option<Arc<neo_rpc::application_logs::ApplicationLogsService<S>>>,
        tokens_tracker: Option<Arc<neo_rpc::plugins::tokens_tracker::TokensTrackerService<S>>>,
        remote_ledger: Option<Arc<RemoteLedgerStatus>>,
    ) -> Self {
        Self {
            state_store,
            state_commit_handlers,
            indexer,
            application_logs,
            tokens_tracker,
            remote_ledger,
        }
    }

    /// Creates an empty handle bundle.
    #[cfg(test)]
    pub(in crate::node) fn empty() -> Self {
        Self::new(None, None, None, None, None, None)
    }

    /// Returns the state-service store.
    pub(in crate::node) fn state_store(&self) -> Option<Arc<neo_state_service::StateStore<S>>> {
        self.state_store.as_ref().map(Arc::clone)
    }

    /// Returns the state-service commit worker.
    pub(in crate::node) fn state_commit_handlers(
        &self,
    ) -> Option<Arc<neo_state_service::commit_handlers::StateServiceCommitHandlers<S>>> {
        self.state_commit_handlers.as_ref().map(Arc::clone)
    }

    /// Returns the indexer service.
    pub(in crate::node) fn indexer(&self) -> Option<Arc<neo_indexer::IndexerService>> {
        self.indexer.as_ref().map(Arc::clone)
    }

    /// Returns the application-log service.
    pub(in crate::node) fn application_logs(
        &self,
    ) -> Option<Arc<neo_rpc::application_logs::ApplicationLogsService<S>>> {
        self.application_logs.as_ref().map(Arc::clone)
    }

    /// Returns the token-tracker service.
    pub(in crate::node) fn tokens_tracker(
        &self,
    ) -> Option<Arc<neo_rpc::plugins::tokens_tracker::TokensTrackerService<S>>> {
        self.tokens_tracker.as_ref().map(Arc::clone)
    }

    /// Returns remote-ledger status when RPC-backed ledger mode is active.
    pub(in crate::node) fn remote_ledger(&self) -> Option<Arc<RemoteLedgerStatus>> {
        self.remote_ledger.as_ref().map(Arc::clone)
    }

    /// Builds the lock-free RPC view over the same service instances.
    pub(in crate::node) fn rpc_services(&self) -> neo_rpc::server::RpcServices<S> {
        let mut services = neo_rpc::server::RpcServices::new();
        if let Some(service) = self.state_store() {
            services = services.with_state_store(service);
        }
        if let Some(service) = self.indexer() {
            services = services.with_indexer(service);
        }
        if let Some(service) = self.application_logs() {
            services = services.with_application_logs(service);
        }
        if let Some(service) = self.tokens_tracker() {
            services = services.with_tokens_tracker(service);
        }
        services
    }
}

impl<S> Clone for NodeServiceHandles<S>
where
    S: Store,
{
    fn clone(&self) -> Self {
        Self::new(
            self.state_store(),
            self.state_commit_handlers(),
            self.indexer(),
            self.application_logs(),
            self.tokens_tracker(),
            self.remote_ledger(),
        )
    }
}

impl<S> std::fmt::Debug for NodeServiceHandles<S>
where
    S: Store,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeServiceHandles")
            .field("state_store", &self.state_store.is_some())
            .field(
                "state_commit_handlers",
                &self.state_commit_handlers.is_some(),
            )
            .field("indexer", &self.indexer.is_some())
            .field("application_logs", &self.application_logs.is_some())
            .field("tokens_tracker", &self.tokens_tracker.is_some())
            .field("remote_ledger", &self.remote_ledger.is_some())
            .finish()
    }
}
