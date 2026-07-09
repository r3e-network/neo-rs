//! # neo-node::node::context
//!
//! Runtime context records carried through the local workflow.
//!
//! ## Boundary
//!
//! This module belongs to `neo-node`. This application crate may compose lower
//! layers but must not define protocol bytes, storage formats, consensus rules,
//! or VM semantics.
//!
//! ## Contents
//!
//! - `context`: daemon runtime context shared by node startup steps.
//! - `plugins`: read-side plugin and deferred commit-hook dispatch.
//! - `system_context`: blockchain service context trait implementation.

use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_storage::persistence::{DataCache, StoreCache};
use parking_lot::{Mutex, RwLock};

mod plugins;
mod system_context;

/// [`neo_blockchain::service_context::SystemContext`] for the daemon:
/// protocol settings plus the canonical store snapshot the blockchain service
/// persists blocks into (and verifies transactions against).
pub(super) struct DaemonContext<P> {
    settings: Arc<ProtocolSettings>,
    snapshot: Arc<DataCache>,
    /// The store-backed cache whose `DataCache` shares state with `snapshot`
    /// (cloned from it). `commit()` flushes block writes to durable storage.
    store_cache: Mutex<StoreCache>,
    state_service: Option<Arc<neo_state_service::commit_handlers::StateServiceCommitHandlers>>,
    state_service_track_during_catchup: bool,
    indexer_service: Option<Arc<neo_indexer::IndexerService>>,
    native_contract_provider: Arc<P>,
    node: RwLock<Option<Arc<neo_system::Node>>>,
    application_logs_service: Option<Arc<neo_rpc::application_logs::ApplicationLogsService>>,
    tokens_tracker: RwLock<Option<Arc<neo_rpc::plugins::tokens_tracker::TokensTracker>>>,
}

impl<P> std::fmt::Debug for DaemonContext<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonContext")
            .field("network", &self.settings.network)
            .finish_non_exhaustive()
    }
}

impl<P> DaemonContext<P>
where
    P: NativeContractProvider + 'static,
{
    pub(super) fn new(
        settings: Arc<ProtocolSettings>,
        snapshot: Arc<DataCache>,
        store_cache: StoreCache,
        state_service: Option<Arc<neo_state_service::commit_handlers::StateServiceCommitHandlers>>,
        state_service_track_during_catchup: bool,
        indexer_service: Option<Arc<neo_indexer::IndexerService>>,
        native_contract_provider: Arc<P>,
        application_logs_service: Option<Arc<neo_rpc::application_logs::ApplicationLogsService>>,
    ) -> Self {
        Self {
            settings,
            snapshot,
            store_cache: Mutex::new(store_cache),
            state_service,
            state_service_track_during_catchup,
            indexer_service,
            native_contract_provider,
            node: RwLock::new(None),
            application_logs_service,
            tokens_tracker: RwLock::new(None),
        }
    }

    pub(super) fn set_node(&self, node: Arc<neo_system::Node>) {
        *self.node.write() = Some(node);
    }

    pub(super) fn set_tokens_tracker(
        &self,
        tokens_tracker: Option<Arc<neo_rpc::plugins::tokens_tracker::TokensTracker>>,
    ) {
        *self.tokens_tracker.write() = tokens_tracker;
    }

    pub(super) fn tokens_tracker(
        &self,
    ) -> Option<Arc<neo_rpc::plugins::tokens_tracker::TokensTracker>> {
        self.tokens_tracker.read().as_ref().map(Arc::clone)
    }
}
