use super::transaction::SystemStoreTransaction;
use super::write_back::EmptyBlockWriteBack;
use super::*;

pub(super) struct SystemSnapshotStore {
    store: Arc<dyn Store>,
    commit_profiler: Option<Arc<dyn StoreCommitProfiler>>,
    empty_block_batch: Option<Arc<EmptyBlockWriteBack>>,
}

impl SystemSnapshotStore {
    pub(super) fn new(
        store: Arc<dyn Store>,
        commit_profiler: Option<Arc<dyn StoreCommitProfiler>>,
    ) -> Self {
        Self {
            store,
            commit_profiler,
            empty_block_batch: None,
        }
    }

    pub(super) fn for_empty_block_batch(
        store: Arc<dyn Store>,
        commit_profiler: Option<Arc<dyn StoreCommitProfiler>>,
        expected_blocks: usize,
    ) -> Self {
        let write_back =
            EmptyBlockWriteBack::new(store.clone(), commit_profiler.clone(), expected_blocks);
        Self {
            store,
            commit_profiler,
            empty_block_batch: Some(Arc::new(write_back)),
        }
    }

    pub(super) fn finish_empty_block_batch(self) -> Result<(), String> {
        let batch = self
            .empty_block_batch
            .ok_or_else(|| "empty-block write-back is not active".to_string())?;
        let batch = Arc::try_unwrap(batch)
            .map_err(|_| "empty-block write-back still has live transaction handles".to_string())?;
        batch.finish()
    }
}

impl SnapshotStore for SystemSnapshotStore {
    fn open_transaction(
        &self,
        _options: &PersistOptions,
    ) -> Result<Box<dyn StoreTransactionPort>, LedgerError> {
        if let Some(batch) = &self.empty_block_batch {
            return Ok(Box::new(SystemStoreTransaction {
                transaction: None,
                base_snapshot: Arc::clone(&batch.base_snapshot),
                empty_block_batch: Some(Arc::clone(batch)),
            }));
        }

        let config = DataCacheConfig {
            track_reads_in_write_cache: true,
            enable_read_cache: false,
            ..Default::default()
        };
        let mut transaction =
            StoreTransaction::from_snapshot_with_config(self.store.snapshot(), config);
        transaction.set_commit_profiler(self.commit_profiler.clone());
        let base_snapshot = Arc::new(transaction.cache().data_cache().clone());
        Ok(Box::new(SystemStoreTransaction {
            transaction: Some(transaction),
            base_snapshot,
            empty_block_batch: None,
        }))
    }
}
