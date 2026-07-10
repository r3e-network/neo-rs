//! Sync import pipeline composition handle.
//!
//! This module belongs to `neo-system`: it wires already-defined runtime
//! primitives to concrete node handles, but it does not download blocks or
//! duplicate blockchain import rules.

use std::sync::Arc;

use neo_blockchain::BlockchainHandle;
use neo_runtime::{
    BlockImportQueue, BlockOrigin, CommitPolicy, SharedStoreSyncStageCheckpointStore,
    SyncPipelineDriver, SyncStageCheckpointStore,
};
use neo_storage::persistence::store::Store;

/// Default number of concurrent stateless block checks for sync import.
///
/// The value follows available CPU parallelism while keeping the preverify
/// queue bounded so a peer burst cannot spawn unbounded tasks.
#[must_use]
pub fn default_sync_import_preverify_concurrency() -> usize {
    std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1)
        .clamp(1, 32)
}

/// Default import-stage checkpoint cadence in blocks.
pub const DEFAULT_SYNC_IMPORT_CHECKPOINT_BLOCKS: u64 = 512;

/// Composed import-stage sync pipeline entry point.
///
/// `neo-runtime` owns the reusable queue, commit policy, and checkpoint driver.
/// This handle binds those primitives to the node's concrete
/// [`BlockchainHandle`] and shared storage provider. Production downloader
/// integration can create short-lived [`SyncPipelineDriver`] values from this
/// handle without reaching into `neo-blockchain` command internals.
#[derive(Clone)]
pub struct SyncImportPipeline<C = SharedStoreSyncStageCheckpointStore>
where
    C: SyncStageCheckpointStore,
{
    import_queue: Arc<BlockImportQueue<BlockchainHandle>>,
    checkpoints: Arc<C>,
    commit_policy: CommitPolicy,
    origin: BlockOrigin,
}

impl<C> std::fmt::Debug for SyncImportPipeline<C>
where
    C: SyncStageCheckpointStore,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncImportPipeline")
            .field("import_queue", &"BlockImportQueue<BlockchainHandle>")
            .field("max_concurrency", &self.import_queue.max_concurrency())
            .field("commit_policy", &self.commit_policy)
            .field("origin", &self.origin)
            .finish_non_exhaustive()
    }
}

impl<S> SyncImportPipeline<SharedStoreSyncStageCheckpointStore<S>>
where
    S: Store + 'static,
{
    /// Compose the sync import pipeline from the canonical blockchain handle
    /// and shared node storage.
    #[must_use]
    pub fn new(blockchain: BlockchainHandle, storage: Arc<S>) -> Self {
        let checkpoints = Arc::new(SharedStoreSyncStageCheckpointStore::new(storage));
        Self::with_parts(
            Arc::new(BlockImportQueue::new(
                Arc::new(blockchain),
                default_sync_import_preverify_concurrency(),
            )),
            checkpoints,
            CommitPolicy::new().with_max_blocks(DEFAULT_SYNC_IMPORT_CHECKPOINT_BLOCKS),
            BlockOrigin::Sync,
        )
    }
}

impl<C> SyncImportPipeline<C>
where
    C: SyncStageCheckpointStore,
{
    /// Compose from explicit parts for tests and specialized node profiles.
    #[must_use]
    pub fn with_parts(
        import_queue: Arc<BlockImportQueue<BlockchainHandle>>,
        checkpoints: Arc<C>,
        commit_policy: CommitPolicy,
        origin: BlockOrigin,
    ) -> Self {
        Self {
            import_queue,
            checkpoints,
            commit_policy,
            origin,
        }
    }

    /// Returns the bounded preverification queue.
    #[must_use]
    pub fn import_queue(&self) -> Arc<BlockImportQueue<BlockchainHandle>> {
        Arc::clone(&self.import_queue)
    }

    /// Returns the durable checkpoint provider used by import-stage drivers.
    #[must_use]
    pub fn checkpoint_store(&self) -> Arc<C> {
        Arc::clone(&self.checkpoints)
    }

    /// Returns the commit policy used when creating import-stage drivers.
    #[must_use]
    pub const fn commit_policy(&self) -> CommitPolicy {
        self.commit_policy
    }

    /// Returns the semantic block origin attached to imported sync batches.
    #[must_use]
    pub const fn origin(&self) -> BlockOrigin {
        self.origin
    }

    /// Create an ordered sync import driver from the latest durable checkpoint.
    pub fn driver(
        &self,
    ) -> neo_runtime::ServiceResult<SyncPipelineDriver<BlockImportQueue<BlockchainHandle>, C>> {
        SyncPipelineDriver::new(
            Arc::clone(&self.import_queue),
            Arc::clone(&self.checkpoints),
            self.commit_policy,
            self.origin,
        )
    }
}
