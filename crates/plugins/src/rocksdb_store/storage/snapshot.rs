use super::options;
use super::store::Store;
use neo_core::persistence::i_read_only_store::{IReadOnlyStore, IReadOnlyStoreGeneric};
use neo_core::persistence::i_store::IStore;
use neo_core::persistence::i_store_snapshot::IStoreSnapshot;
use neo_core::persistence::i_write_store::IWriteStore;
use neo_core::persistence::seek_direction::SeekDirection;
use neo_core::smart_contract::{StorageItem, StorageKey};
use rocksdb::{DBIteratorWithThreadMode, IteratorMode, ReadOptions, WriteBatch, DB};
use std::mem;
use std::sync::{Arc, Mutex};

pub struct Snapshot {
    store: Arc<Store>,
    db: Arc<DB>,
    snapshot: rocksdb::Snapshot<'static>,
    write_batch: Mutex<WriteBatch>,
}

impl Snapshot {
    pub fn new(db: Arc<DB>, store: Arc<Store>) -> Self {
        let snapshot = db.snapshot();
        // SAFETY: the snapshot is dropped before the database because we hold an Arc to the DB.
        let snapshot = unsafe {
            mem::transmute::<rocksdb::Snapshot<'_>, rocksdb::Snapshot<'static>>(snapshot)
        };

        Self {
            store,
            db,
            snapshot,
            write_batch: Mutex::new(WriteBatch::default()),
        }
    }

    fn read_options(&self) -> ReadOptions {
        options::read_options_with_snapshot(&self.snapshot)
    }

    fn iterator_from(
        &self,
        key_or_prefix: &[u8],
        prefix: Option<Vec<u8>>,
        direction: SeekDirection,
    ) -> SnapshotIterator<'_> {
        let read_options = self.read_options();
        SnapshotIterator::new(
            self.db.as_ref(),
            read_options,
            key_or_prefix,
            prefix,
            direction,
        )
    }
}

struct SnapshotIterator<'a> {
    inner: DBIteratorWithThreadMode<'a, DB>,
    prefix: Option<Vec<u8>>,
    done: bool,
}

impl<'a> SnapshotIterator<'a> {
    fn new(
        db: &'a DB,
        read_options: ReadOptions,
        key_or_prefix: &[u8],
        prefix: Option<Vec<u8>>,
        direction: SeekDirection,
    ) -> Self {
        let mode = match direction {
            SeekDirection::Forward => {
                if key_or_prefix.is_empty() {
                    IteratorMode::Start
                } else {
                    IteratorMode::From(key_or_prefix, rocksdb::Direction::Forward)
                }
            }
            SeekDirection::Backward => {
                if key_or_prefix.is_empty() {
                    IteratorMode::End
                } else {
                    IteratorMode::From(key_or_prefix, rocksdb::Direction::Reverse)
                }
            }
        };

        let inner = db.iterator_opt(mode, read_options);
        Self {
            inner,
            prefix,
            done: false,
        }
    }

    fn matches_prefix(&self, key: &[u8]) -> bool {
        if let Some(prefix) = &self.prefix {
            key.starts_with(prefix)
        } else {
            true
        }
    }
}

impl Iterator for SnapshotIterator<'_> {
    type Item = (Vec<u8>, Vec<u8>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }

        while let Some(result) = self.inner.next() {
            match result {
                Ok((key, value)) => {
                    let key_vec = key.to_vec();
                    if self.matches_prefix(&key_vec) {
                        return Some((key_vec, value.to_vec()));
                    }

                    if self.prefix.is_some() {
                        self.done = true;
                        return None;
                    }
                }
                Err(_) => {
                    self.done = true;
                    return None;
                }
            }
        }

        None
    }
}

impl IReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for Snapshot {
    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        self.db
            .get_opt(key, &self.read_options())
            .expect("RocksDB snapshot get failed")
    }

    fn contains(&self, key: &Vec<u8>) -> bool {
        self.db
            .get_pinned_opt(key, &self.read_options())
            .expect("RocksDB snapshot contains check failed")
            .is_some()
    }

    fn find(
        &self,
        key_prefix: Option<&Vec<u8>>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        let prefix = key_prefix.cloned();
        let start_prefix = prefix.clone();
        let start = start_prefix.as_deref().unwrap_or(&[]);
        let iterator = self.iterator_from(start, prefix, direction);
        Box::new(iterator)
    }
}

impl IReadOnlyStoreGeneric<StorageKey, StorageItem> for Snapshot {
    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        let raw = key.to_array();
        self.db
            .get_opt(raw, &self.read_options())
            .ok()
            .flatten()
            .map(StorageItem::from_bytes)
    }

    fn contains(&self, key: &StorageKey) -> bool {
        let raw = key.to_array();
        self.db
            .get_pinned_opt(raw, &self.read_options())
            .ok()
            .flatten()
            .is_some()
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        let prefix_bytes = key_prefix.map(|k| k.to_array());
        let start = prefix_bytes.as_deref().unwrap_or(&[]);
        let iter = self.iterator_from(start, prefix_bytes.clone(), direction);
        Box::new(iter.filter_map(move |(key, value)| {
            if let Some(prefix) = &prefix_bytes {
                if !key.starts_with(prefix) {
                    return None;
                }
            }
            Some((StorageKey::from_bytes(&key), StorageItem::from_bytes(value)))
        }))
    }
}

impl IReadOnlyStore for Snapshot {}

impl IWriteStore<Vec<u8>, Vec<u8>> for Snapshot {
    fn delete(&mut self, key: Vec<u8>) {
        let mut batch = self.write_batch.lock().unwrap();
        batch.delete(key);
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) {
        let mut batch = self.write_batch.lock().unwrap();
        batch.put(key, value);
    }
}

impl IStoreSnapshot for Snapshot {
    fn store(&self) -> Arc<dyn IStore> {
        self.store.clone() as Arc<dyn IStore>
    }

    fn commit(&mut self) {
        let mut batch_guard = self.write_batch.lock().unwrap();
        if batch_guard.len() == 0 {
            return;
        }

        let mut write_batch = WriteBatch::default();
        mem::swap(&mut *batch_guard, &mut write_batch);
        drop(batch_guard);

        let write_options = options::write_options();
        self.db
            .write_opt(write_batch, &write_options)
            .expect("Failed to commit RocksDB snapshot");
    }
}
