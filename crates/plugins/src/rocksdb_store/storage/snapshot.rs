use super::options;
use super::store::Store;
use neo_core::persistence::i_read_only_store::IReadOnlyStoreGeneric;
use neo_core::persistence::i_store::IStore;
use neo_core::persistence::i_store_snapshot::IStoreSnapshot;
use neo_core::persistence::i_write_store::IWriteStore;
use neo_core::persistence::seek_direction::SeekDirection;
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
        direction: SeekDirection,
    ) -> SnapshotIterator<'_> {
        let read_options = self.read_options();
        SnapshotIterator::new(self.db.as_ref(), read_options, key_or_prefix, direction)
    }
}

struct SnapshotIterator<'a> {
    inner: DBIteratorWithThreadMode<'a, DB>,
    direction: SeekDirection,
}

impl<'a> SnapshotIterator<'a> {
    fn new(
        db: &'a DB,
        read_options: ReadOptions,
        key_or_prefix: &[u8],
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
        Self { inner, direction }
    }
}

impl<'a> Iterator for SnapshotIterator<'a> {
    type Item = (Vec<u8>, Vec<u8>);

    fn next(&mut self) -> Option<Self::Item> {
        if !self.inner.valid() {
            return None;
        }

        let key = self.inner.key().to_vec();
        let value = self.inner.value().to_vec();

        match self.direction {
            SeekDirection::Forward => {
                self.inner.next();
            }
            SeekDirection::Backward => {
                self.inner.prev();
            }
        }

        Some((key, value))
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
        let start = key_prefix.map(|k| k.as_slice()).unwrap_or(&[]);
        let iterator = self.iterator_from(start, direction);
        Box::new(iterator)
    }
}

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
