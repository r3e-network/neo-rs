use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_payloads::{ApplicationExecuted, Block, CommittedHandler, CommittingHandler};
use neo_storage::persistence::{DataCache, StoreCache};
use parking_lot::{Mutex, RwLock};
use tracing::warn;

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
    indexer_service: Option<Arc<neo_indexer::IndexerService>>,
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
        indexer_service: Option<Arc<neo_indexer::IndexerService>>,
        application_logs_service: Option<Arc<neo_rpc::application_logs::ApplicationLogsService>>,
    ) -> Self {
        Self {
            settings,
            snapshot,
            store_cache: Mutex::new(store_cache),
            state_service,
            indexer_service,
            node: RwLock::new(None),
            application_logs_service,
            tokens_tracker: RwLock::new(None),
        }
    }

    pub(super) fn set_node(&self, node: Arc<neo_system::Node>) {
        *self.node.write() = Some(node);
    }

    /// Returns the best-known live chain tip height reported by peers.
    /// Returns 0 if no peer has reported a height yet.
    pub fn live_tip_height(&self) -> u64 {
        neo_runtime::sync_metrics::peer_live_tip()
    }

    pub(super) fn set_tokens_tracker(
        &self,
        tokens_tracker: Option<Arc<neo_rpc::plugins::tokens_tracker::TokensTracker>>,
    ) {
        *self.tokens_tracker.write() = tokens_tracker;
    }

    fn plugin_node(&self) -> Option<Arc<neo_system::Node>> {
        self.node.read().as_ref().map(Arc::clone)
    }

    fn tokens_tracker(&self) -> Option<Arc<neo_rpc::plugins::tokens_tracker::TokensTracker>> {
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

    fn block_committing(
        &self,
        block: &Block,
        snapshot: &DataCache,
        application_executed_list: &[ApplicationExecuted],
    ) -> bool {
        // During catch-up, skip the expensive per-block hooks:
        // - StateService.on_committing computes the MPT state root per block
        //   (~24ms measured — the dominant sync bottleneck). State roots are
        //   only needed for the state-service RPC, not for consensus.
        // - IndexerService.index_block indexes transaction execution results.
        // Both resume near the live tip. This mirrors C# Neo's chain.acc
        // import which skips verification and indexing during bulk sync.
        let block_index = block.index();
        let live_tip = neo_runtime::sync_metrics::peer_live_tip();
        let catching_up = live_tip > 0 && (block_index as u64) + 10000 < live_tip;
        if catching_up {
            return true;
        }

        if let Some(state_service) = &self.state_service {
            if !state_service.on_committing(block.index(), snapshot) {
                return false;
            }
        }

        if let Some(indexer) = &self.indexer_service {
            if let Err(error) =
                indexer.index_block_with_application_executions(block, application_executed_list)
            {
                warn!(
                    target: "neo::indexer",
                    height = block.index(),
                    error = %error,
                    "failed to index block application executions"
                );
            }
        }

        self.commit_plugin_committing_handlers(block, snapshot, application_executed_list);
        true
    }

    fn block_committed(&self, block: &Block) {
        // During initial catch-up, skip the application-logs and tokens-tracker
        // indexing handlers (C# does the same during chain.acc import). These
        // index every transaction's execution result per block, which is O(N)
        // expensive and dominates sync time (measured: 30 blocks/min WITH
        // indexing vs 200+ WITHOUT). Once the node is near the live tip
        // (within ~1000 blocks), we enable full indexing for live operation.
        //
        // The plugin services can backfill later via their own catch-up path
        // if needed; the priority during cold sync is reaching consensus tip.
        let block_index = block.index();
        let live_tip = self.live_tip_height();
        let catching_up = live_tip > 0 && (block_index as u64) + 1000 < live_tip;
        if catching_up {
            return;
        }

        let application_logs = self.application_logs_service.as_ref().map(Arc::clone);
        let tokens_tracker = self.tokens_tracker();
        if application_logs.is_none() && tokens_tracker.is_none() {
            return;
        }
        let Some(node) = self.plugin_node() else {
            return;
        };

        if let Some(application_logs) = application_logs {
            application_logs.blockchain_committed_handler(node.as_ref(), block);
        }
        if let Some(tokens_tracker) = tokens_tracker {
            tokens_tracker.blockchain_committed_handler(node.as_ref(), block);
        }
    }

    fn commit_to_store(&self) {
        // The StoreCache's DataCache shares state with `snapshot` (it was cloned
        // from it), so its tracked block writes are flushed through to the store.
        self.store_cache.lock().commit();
    }
}

impl DaemonContext {
    fn commit_plugin_committing_handlers(
        &self,
        block: &Block,
        snapshot: &DataCache,
        application_executed_list: &[ApplicationExecuted],
    ) {
        let application_logs = self.application_logs_service.as_ref().map(Arc::clone);
        let tokens_tracker = self.tokens_tracker();
        if application_logs.is_none() && tokens_tracker.is_none() {
            return;
        }
        let Some(node) = self.plugin_node() else {
            return;
        };

        if let Some(application_logs) = application_logs {
            application_logs.blockchain_committing_handler(
                node.as_ref(),
                block,
                snapshot,
                application_executed_list,
            );
        }
        if let Some(tokens_tracker) = tokens_tracker {
            tokens_tracker.blockchain_committing_handler(
                node.as_ref(),
                block,
                snapshot,
                application_executed_list,
            );
        }
    }
}
