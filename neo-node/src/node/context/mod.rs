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
//! - `plugins`: read-side plugin, static archive publication/pruning, and
//!   deferred commit-hook dispatch.

use std::sync::Arc;

use neo_execution::native_contract_provider::NativeContractProvider;
use neo_storage::persistence::providers::RuntimeStore;
use neo_storage::persistence::providers::memory_store::MemoryStore;
use neo_storage::persistence::store::Store;
use parking_lot::Mutex;
use parking_lot::RwLock;

use super::recovery::LocalReplayGuard;

mod plugins;

#[derive(Clone)]
struct HotLedgerPruning {
    store: Arc<RuntimeStore>,
    retention_blocks: u32,
}

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
    static_archive: Option<neo_blockchain::StaticLedgerArchive>,
    pending_static_records: Mutex<Vec<neo_static_files::StaticRecord>>,
    hot_ledger_pruning: RwLock<Option<HotLedgerPruning>>,
    replay_guard: Arc<LocalReplayGuard>,
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
        static_archive: Option<neo_blockchain::StaticLedgerArchive>,
        replay_guard: Arc<LocalReplayGuard>,
    ) -> Self {
        Self {
            network,
            state_service,
            state_service_track_during_catchup,
            indexer_service,
            application_logs_service,
            tokens_tracker: RwLock::new(None),
            static_archive,
            pending_static_records: Mutex::new(Vec::new()),
            hot_ledger_pruning: RwLock::new(None),
            replay_guard,
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

    pub(super) fn configure_hot_ledger_pruning(
        &self,
        store: Arc<RuntimeStore>,
        retention_blocks: u32,
    ) {
        *self.hot_ledger_pruning.write() = Some(HotLedgerPruning {
            store,
            retention_blocks,
        });
    }
}
