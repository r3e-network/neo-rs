use super::*;
use neo_config::ProtocolSettings;
use neo_native_contracts::StandardNativeProvider;
use neo_storage::persistence::{
    read_only_store::{RawReadOnlyStore, ReadOnlyStore, ReadOnlyStoreGeneric},
    storage::StorageError,
    store_snapshot::StoreSnapshot,
    write_store::WriteStore,
};
use neo_storage::{StorageItem, StorageKey};

#[derive(Clone, Debug)]
struct FailingStore;

impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for FailingStore {
    type FindIterator<'a> = std::vec::IntoIter<(Vec<u8>, Vec<u8>)>;

    fn try_get(&self, _key: &Vec<u8>) -> Option<Vec<u8>> {
        None
    }

    fn find(
        &self,
        _key_prefix: Option<&Vec<u8>>,
        _direction: SeekDirection,
    ) -> Self::FindIterator<'_> {
        Vec::new().into_iter()
    }
}

impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for FailingStore {
    type FindIterator<'a> = std::vec::IntoIter<(StorageKey, StorageItem)>;

    fn try_get(&self, _key: &StorageKey) -> Option<StorageItem> {
        None
    }

    fn find(
        &self,
        _key_prefix: Option<&StorageKey>,
        _direction: SeekDirection,
    ) -> Self::FindIterator<'_> {
        Vec::new().into_iter()
    }
}

impl ReadOnlyStore for FailingStore {}

impl RawReadOnlyStore for FailingStore {
    fn try_get_bytes(&self, _key: &[u8]) -> Option<Vec<u8>> {
        None
    }
}

impl WriteStore<Vec<u8>, Vec<u8>> for FailingStore {
    fn delete(&mut self, _key: Vec<u8>) -> neo_storage::StorageResult<()> {
        Ok(())
    }

    fn put(&mut self, _key: Vec<u8>, _value: Vec<u8>) -> neo_storage::StorageResult<()> {
        Ok(())
    }
}

impl Store for FailingStore {
    type Snapshot = FailingSnapshot;

    fn snapshot(&self) -> Arc<Self::Snapshot> {
        Arc::new(FailingSnapshot {
            store: Arc::new(self.clone()),
        })
    }
}

#[derive(Debug)]
struct FailingSnapshot {
    store: Arc<FailingStore>,
}

impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for FailingSnapshot {
    type FindIterator<'a> = std::vec::IntoIter<(Vec<u8>, Vec<u8>)>;

    fn try_get(&self, _key: &Vec<u8>) -> Option<Vec<u8>> {
        None
    }

    fn find(
        &self,
        _key_prefix: Option<&Vec<u8>>,
        _direction: SeekDirection,
    ) -> Self::FindIterator<'_> {
        Vec::new().into_iter()
    }
}

impl RawReadOnlyStore for FailingSnapshot {
    fn try_get_bytes(&self, _key: &[u8]) -> Option<Vec<u8>> {
        None
    }
}

impl WriteStore<Vec<u8>, Vec<u8>> for FailingSnapshot {
    fn delete(&mut self, _key: Vec<u8>) -> neo_storage::StorageResult<()> {
        Ok(())
    }

    fn put(&mut self, _key: Vec<u8>, _value: Vec<u8>) -> neo_storage::StorageResult<()> {
        Ok(())
    }
}

impl StoreSnapshot for FailingSnapshot {
    type Store = FailingStore;

    fn store(&self) -> Arc<Self::Store> {
        self.store.clone()
    }

    fn try_commit(&mut self) -> neo_storage::persistence::store_snapshot::SnapshotCommitResult {
        Err(StorageError::CommitFailed(
            "injected tracker commit failure".to_string(),
        ))
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn tracker_base_commit_propagates_snapshot_try_commit_failure() {
    let settings = Arc::new(ProtocolSettings::mainnet());
    let mut tracker = TrackerBase::new(
        Arc::new(FailingStore),
        100,
        true,
        settings,
        Arc::new(StandardNativeProvider::new()),
    );
    tracker.reset_batch();

    let err = tracker
        .commit()
        .expect_err("tracker commit should propagate snapshot commit failure");

    assert!(err.to_string().contains("injected tracker commit failure"));
}
