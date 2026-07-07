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

use std::sync::Arc;

use neo_blockchain::service_context::BlockPersistContext;
use neo_config::ProtocolSettings;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_payloads::{ApplicationExecuted, Block};
use neo_storage::persistence::{DataCache, StoreCache};
use parking_lot::{Mutex, RwLock};

mod plugins;

/// [`neo_blockchain::service_context::SystemContext`] for the daemon:
/// protocol settings plus the canonical store snapshot the blockchain service
/// persists blocks into (and verifies transactions against).
pub(super) struct DaemonContext {
    settings: Arc<ProtocolSettings>,
    snapshot: Arc<DataCache>,
    /// The store-backed cache whose `DataCache` shares state with `snapshot`
    /// (cloned from it). `commit()` flushes block writes to durable storage.
    store_cache: Mutex<StoreCache>,
    state_service: Option<Arc<neo_state_service::commit_handlers::StateServiceCommitHandlers>>,
    state_service_track_during_catchup: bool,
    indexer_service: Option<Arc<neo_indexer::IndexerService>>,
    native_contract_provider: Option<Arc<dyn NativeContractProvider>>,
    node: RwLock<Option<Arc<neo_system::Node>>>,
    application_logs_service: Option<Arc<neo_rpc::application_logs::ApplicationLogsService>>,
    tokens_tracker: RwLock<Option<Arc<neo_rpc::plugins::tokens_tracker::TokensTracker>>>,
}

impl std::fmt::Debug for DaemonContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonContext")
            .field("network", &self.settings.network)
            .finish_non_exhaustive()
    }
}

impl DaemonContext {
    pub(super) fn new(
        settings: Arc<ProtocolSettings>,
        snapshot: Arc<DataCache>,
        store_cache: StoreCache,
        state_service: Option<Arc<neo_state_service::commit_handlers::StateServiceCommitHandlers>>,
        state_service_track_during_catchup: bool,
        indexer_service: Option<Arc<neo_indexer::IndexerService>>,
        application_logs_service: Option<Arc<neo_rpc::application_logs::ApplicationLogsService>>,
    ) -> Self {
        Self {
            settings,
            snapshot,
            store_cache: Mutex::new(store_cache),
            state_service,
            state_service_track_during_catchup,
            indexer_service,
            native_contract_provider: None,
            node: RwLock::new(None),
            application_logs_service,
            tokens_tracker: RwLock::new(None),
        }
    }

    pub(super) fn with_native_contract_provider(
        mut self,
        native_contract_provider: Arc<dyn NativeContractProvider>,
    ) -> Self {
        self.native_contract_provider = Some(native_contract_provider);
        self
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

impl neo_blockchain::service_context::SystemContext for DaemonContext {
    fn settings(&self) -> Arc<ProtocolSettings> {
        Arc::clone(&self.settings)
    }

    fn current_height(&self) -> u32 {
        neo_native_contracts::LedgerContract::new()
            .current_index(&self.snapshot)
            .unwrap_or(0)
    }

    fn store_snapshot(&self) -> Option<Arc<DataCache>> {
        Some(Arc::clone(&self.snapshot))
    }

    fn native_contract_provider(&self) -> Option<Arc<dyn NativeContractProvider>> {
        self.native_contract_provider.as_ref().map(Arc::clone)
    }

    fn block_committing(
        &self,
        block: &Block,
        snapshot: &DataCache,
        application_executed_list: &[ApplicationExecuted],
    ) -> bool {
        self.block_committing_with_live_tip(
            block,
            snapshot,
            application_executed_list,
            neo_runtime::sync_metrics::peer_live_tip(),
        )
    }

    fn block_committing_with_context(
        &self,
        block: &Block,
        snapshot: &DataCache,
        application_executed_list: &[ApplicationExecuted],
        context: BlockPersistContext,
    ) -> bool {
        self.block_committing_with_live_tip_and_context(
            block,
            snapshot,
            application_executed_list,
            neo_runtime::sync_metrics::peer_live_tip(),
            context,
        )
    }

    fn block_committed(&self, block: &Block) {
        self.block_committed_with_live_tip_and_context(
            block,
            neo_runtime::sync_metrics::peer_live_tip(),
            BlockPersistContext::live(),
        );
    }

    fn block_committed_with_context(&self, block: &Block, context: BlockPersistContext) {
        self.block_committed_with_live_tip_and_context(
            block,
            neo_runtime::sync_metrics::peer_live_tip(),
            context,
        );
    }

    fn commit_to_store(&self) {
        // The StoreCache's DataCache shares state with `snapshot` (it was cloned
        // from it), so its tracked block writes are flushed through to the store.
        self.store_cache.lock().commit();
    }

    fn flush_bulk_sync_commit_handlers(&self) -> Result<(), String> {
        if let Some(state_service) = &self.state_service {
            state_service
                .flush_result()
                .map_err(|err| err.to_string())?;
        }
        Ok(())
    }

    fn allows_empty_block_fast_forward(&self) -> bool {
        self.state_service.is_none()
            && self.indexer_service.is_none()
            && self.application_logs_service.is_none()
            && self.tokens_tracker().is_none()
    }

    fn allows_empty_block_committing_fast_forward(&self) -> bool {
        self.state_service.is_some()
            && self.indexer_service.is_none()
            && self.application_logs_service.is_none()
            && self.tokens_tracker().is_none()
    }
}
