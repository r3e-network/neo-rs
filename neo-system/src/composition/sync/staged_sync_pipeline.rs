//! Node-composed staged sync entry point.
//!
//! The top-level handle exposes protocol intent in order: verify and persist
//! headers, fetch bodies through the verified-header gate, then hand ordered
//! blocks to the existing canonical import pipeline.

use std::sync::Arc;

use neo_blockchain::{BlockchainHandle, HeaderCache};
use neo_network::BlockRangeFetcher;
use neo_runtime::{
    SharedStoreSyncStageCheckpointStore, SharedStoreVerifiedHeaderStore, SyncStageCheckpointStore,
    VerifiedHeaderStore,
};
use neo_storage::persistence::TransactionalStore;

use crate::sync_header_pipeline::SyncHeaderPipeline;
use crate::sync_import_pipeline::SyncImportPipeline;
use crate::verified_block_fetcher::VerifiedBlockRangeFetcher;

/// Statically composed `Headers -> Bodies -> Import` pipeline.
pub struct StagedSyncPipeline<C, H>
where
    C: SyncStageCheckpointStore,
    H: VerifiedHeaderStore,
{
    headers: Arc<SyncHeaderPipeline<H>>,
    import: Arc<SyncImportPipeline<C>>,
}

impl<C, H> std::fmt::Debug for StagedSyncPipeline<C, H>
where
    C: SyncStageCheckpointStore,
    H: VerifiedHeaderStore,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StagedSyncPipeline")
            .field("headers", &self.headers)
            .field("import", &self.import)
            .finish()
    }
}

impl<S>
    StagedSyncPipeline<SharedStoreSyncStageCheckpointStore<S>, SharedStoreVerifiedHeaderStore<S>>
where
    S: TransactionalStore + 'static,
{
    /// Compose all production sync stages over the canonical blockchain handle,
    /// shared header cache, and shared storage backend.
    #[must_use]
    pub fn new(
        blockchain: BlockchainHandle,
        header_cache: Arc<HeaderCache>,
        storage: Arc<S>,
    ) -> Self {
        let headers = Arc::new(SyncHeaderPipeline::new(
            blockchain.clone(),
            header_cache,
            Arc::new(SharedStoreVerifiedHeaderStore::new(Arc::clone(&storage))),
        ));
        let import = Arc::new(SyncImportPipeline::new(blockchain, storage));
        Self::with_parts(headers, import)
    }
}

impl<C, H> StagedSyncPipeline<C, H>
where
    C: SyncStageCheckpointStore,
    H: VerifiedHeaderStore,
{
    /// Compose explicit stage handles for tests and specialized profiles.
    #[must_use]
    pub fn with_parts(
        headers: Arc<SyncHeaderPipeline<H>>,
        import: Arc<SyncImportPipeline<C>>,
    ) -> Self {
        Self { headers, import }
    }

    /// Returns the durable header-stage handle.
    #[must_use]
    pub fn headers(&self) -> Arc<SyncHeaderPipeline<H>> {
        Arc::clone(&self.headers)
    }

    /// Returns the canonical import-stage handle.
    #[must_use]
    pub fn import(&self) -> Arc<SyncImportPipeline<C>> {
        Arc::clone(&self.import)
    }

    /// Wrap a transport fetcher so body/header disagreement participates in
    /// the coordinator's normal cross-peer retry policy.
    #[must_use]
    pub fn verified_fetcher<F>(&self, fetcher: F) -> VerifiedBlockRangeFetcher<F, H>
    where
        F: BlockRangeFetcher,
    {
        VerifiedBlockRangeFetcher::new(fetcher, self.headers())
    }
}
