//! # neo-node::node::context
//!
//! Application-owned durability hooks and finalized projections.
//!
//! ## Boundary
//!
//! This module belongs to `neo-node`. This application crate may compose lower
//! layers but must not define protocol bytes, storage formats, consensus rules,
//! or VM semantics.
//!
//! ## Contents
//!
//! - `plugins`: pre-commit durability policy, finalized publication, static
//!   archive publication/pruning, and deferred-hook dispatch.
//! - `finality`: acknowledged ApplicationLogs and TokensTracker projections.

use std::sync::Arc;

use neo_execution::native_contract_provider::NativeContractProvider;
use neo_storage::persistence::Store;
use neo_storage::persistence::providers::RuntimeStore;
use neo_storage::persistence::providers::memory_store::MemoryStore;
use parking_lot::{Mutex, RwLock};

use super::recovery::LocalReplayGuard;

mod finality;
mod plugins;

pub(in crate::node) use finality::FinalizedProjectionConsumer;

#[derive(Clone)]
struct HotLedgerPruning {
    store: Arc<RuntimeStore>,
    retention_blocks: u32,
}

/// Application observers and catch-up policy used by the core system context.
pub(super) struct DaemonCommitHooks<P, L: Store = MemoryStore, T: Store = MemoryStore>
where
    P: NativeContractProvider,
{
    state_service:
        Option<Arc<neo_state_service::commit_handlers::StateServiceCommitHandlers<RuntimeStore>>>,
    state_service_track_during_catchup: bool,
    indexer_service: Option<Arc<neo_indexer::IndexerService>>,
    finalized_projections: Arc<FinalizedProjectionConsumer<P, L, T>>,
    finalized_blocks:
        neo_system::FinalizedBlockHandle<neo_storage::persistence::StoreCacheBacking<RuntimeStore>>,
    static_archive: Option<neo_blockchain::StaticLedgerArchive>,
    pending_static_records: Mutex<Vec<neo_static_files::StaticRecord>>,
    hot_ledger_pruning: RwLock<Option<HotLedgerPruning>>,
    replay_guard: Arc<LocalReplayGuard>,
    append_shadow: Option<Arc<crate::node::append_shadow::AppendShadow>>,
    authoritative_pack: Option<Arc<crate::node::state_packs::AuthoritativeNodePack>>,
}

impl<P, L, T> std::fmt::Debug for DaemonCommitHooks<P, L, T>
where
    P: NativeContractProvider,
    L: Store,
    T: Store,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonCommitHooks")
            .field("finalized_projections", &self.finalized_projections)
            .finish_non_exhaustive()
    }
}

impl<P, L, T> DaemonCommitHooks<P, L, T>
where
    P: NativeContractProvider + 'static,
    L: Store + 'static,
    T: Store + 'static,
{
    pub(super) fn compose(
        network: u32,
        state_service: Option<
            Arc<neo_state_service::commit_handlers::StateServiceCommitHandlers<RuntimeStore>>,
        >,
        state_service_track_during_catchup: bool,
        indexer_service: Option<Arc<neo_indexer::IndexerService>>,
        application_logs_service: Option<Arc<neo_rpc::application_logs::ApplicationLogsService<L>>>,
        tokens_tracker: Option<Arc<neo_rpc::plugins::tokens_tracker::TokensTracker<P, T>>>,
        static_archive: Option<neo_blockchain::StaticLedgerArchive>,
        replay_guard: Arc<LocalReplayGuard>,
        append_shadow: Option<Arc<crate::node::append_shadow::AppendShadow>>,
        authoritative_pack: Option<Arc<crate::node::state_packs::AuthoritativeNodePack>>,
    ) -> (
        Arc<Self>,
        neo_system::FinalizedBlockStream<
            neo_storage::persistence::StoreCacheBacking<RuntimeStore>,
            FinalizedProjectionConsumer<P, L, T>,
        >,
    ) {
        let finalized_projections = Arc::new(FinalizedProjectionConsumer::new(
            network,
            application_logs_service,
            tokens_tracker,
        ));
        let (finalized_blocks, finalized_stream) =
            neo_system::FinalizedBlockStreamFactory::default()
                .for_backing::<neo_storage::persistence::StoreCacheBacking<RuntimeStore>>()
                .create(Arc::clone(&finalized_projections));
        let hooks = Arc::new(Self {
            state_service,
            state_service_track_during_catchup,
            indexer_service,
            finalized_projections,
            finalized_blocks,
            static_archive,
            pending_static_records: Mutex::new(Vec::new()),
            hot_ledger_pruning: RwLock::new(None),
            replay_guard,
            append_shadow,
            authoritative_pack,
        });
        (hooks, finalized_stream)
    }

    #[cfg(test)]
    pub(in crate::node) fn new(
        network: u32,
        state_service: Option<
            Arc<neo_state_service::commit_handlers::StateServiceCommitHandlers<RuntimeStore>>,
        >,
        state_service_track_during_catchup: bool,
        indexer_service: Option<Arc<neo_indexer::IndexerService>>,
        application_logs_service: Option<Arc<neo_rpc::application_logs::ApplicationLogsService<L>>>,
        static_archive: Option<neo_blockchain::StaticLedgerArchive>,
        replay_guard: Arc<LocalReplayGuard>,
    ) -> Self {
        let (hooks, _stream) = Self::compose(
            network,
            state_service,
            state_service_track_during_catchup,
            indexer_service,
            application_logs_service,
            None,
            static_archive,
            replay_guard,
            None,
            None,
        );
        Arc::into_inner(hooks).expect("newly composed commit hooks have one owner")
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
