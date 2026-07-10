//! # neo-node::node::context
//!
//! Application-owned block-commit hooks.
//!
//! ## Boundary
//!
//! This module belongs to `neo-node`. This application crate may compose lower
//! layers but must not define protocol bytes, storage formats, consensus rules,
//! or VM semantics.
//!
//! ## Contents
//!
//! - `plugins`: read-side plugin and deferred commit-hook dispatch.

use std::sync::Arc;

use neo_execution::native_contract_provider::NativeContractProvider;
use neo_storage::persistence::providers::memory_store::MemoryStore;
use neo_storage::persistence::store::Store;
use parking_lot::RwLock;

mod plugins;

/// Application observers and catch-up policy used by the core system context.
pub(super) struct DaemonCommitHooks<
    P,
    S: Store = MemoryStore,
    L: Store = MemoryStore,
    T: Store = MemoryStore,
> where
    P: NativeContractProvider,
{
    network: u32,
    state_service: Option<Arc<neo_state_service::commit_handlers::StateServiceCommitHandlers<S>>>,
    state_service_track_during_catchup: bool,
    indexer_service: Option<Arc<neo_indexer::IndexerService>>,
    application_logs_service: Option<Arc<neo_rpc::application_logs::ApplicationLogsService<L>>>,
    tokens_tracker: RwLock<Option<Arc<neo_rpc::plugins::tokens_tracker::TokensTracker<P, T>>>>,
}

impl<P, S, L, T> std::fmt::Debug for DaemonCommitHooks<P, S, L, T>
where
    P: NativeContractProvider,
    S: Store,
    L: Store,
    T: Store,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonCommitHooks")
            .field("network", &self.network)
            .finish_non_exhaustive()
    }
}

impl<P, S, L, T> DaemonCommitHooks<P, S, L, T>
where
    P: NativeContractProvider + 'static,
    S: Store + 'static,
    L: Store + 'static,
    T: Store + 'static,
{
    pub(super) fn new(
        network: u32,
        state_service: Option<
            Arc<neo_state_service::commit_handlers::StateServiceCommitHandlers<S>>,
        >,
        state_service_track_during_catchup: bool,
        indexer_service: Option<Arc<neo_indexer::IndexerService>>,
        application_logs_service: Option<Arc<neo_rpc::application_logs::ApplicationLogsService<L>>>,
    ) -> Self {
        Self {
            network,
            state_service,
            state_service_track_during_catchup,
            indexer_service,
            application_logs_service,
            tokens_tracker: RwLock::new(None),
        }
    }

    pub(super) fn set_tokens_tracker(
        &self,
        tokens_tracker: Option<Arc<neo_rpc::plugins::tokens_tracker::TokensTracker<P, T>>>,
    ) {
        *self.tokens_tracker.write() = tokens_tracker;
    }

    pub(super) fn tokens_tracker(
        &self,
    ) -> Option<Arc<neo_rpc::plugins::tokens_tracker::TokensTracker<P, T>>> {
        self.tokens_tracker.read().as_ref().map(Arc::clone)
    }
}
