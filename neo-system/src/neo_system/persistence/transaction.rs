use super::write_back::EmptyBlockWriteBack;
use super::*;

pub(super) struct SystemStoreTransaction {
    pub(super) transaction: Option<StoreTransaction>,
    pub(super) base_snapshot: Arc<PersistenceDataCache>,
    pub(super) empty_block_batch: Option<Arc<EmptyBlockWriteBack>>,
}

impl StoreTransactionPort for SystemStoreTransaction {
    fn base_snapshot(&self) -> Arc<PersistenceDataCache> {
        Arc::clone(&self.base_snapshot)
    }

    fn commit(mut self: Box<Self>) -> Result<(), String> {
        if let Some(batch) = self.empty_block_batch {
            return batch.commit_logical_block();
        }
        self.transaction
            .take()
            .expect("store transaction committed once")
            .commit()
            .map_err(|error| error.to_string())
    }
}
