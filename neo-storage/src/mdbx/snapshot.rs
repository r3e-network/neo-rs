use super::store::{MdbxStore, collect_cursor_entries};
use crate::persistence::{
    read_only_store::RawReadOnlyStore,
    read_only_store::ReadOnlyStoreGeneric,
    seek_direction::SeekDirection,
    store::Store,
    store_snapshot::{SnapshotCommitResult, StoreSnapshot},
    write_store::WriteStore,
};
use libmdbx::{NoWriteMap, RO, Transaction};
use parking_lot::RwLock;
use std::{collections::BTreeMap, sync::Arc};
use tracing::{error, warn};

type WriteBatch = Arc<RwLock<BTreeMap<Vec<u8>, Option<Vec<u8>>>>>;

/// Mutable point-in-time snapshot over an MDBX store.
pub struct MdbxSnapshot {
    // Keep this field before `store` so the read transaction is dropped before
    // the database Arc that keeps its widened lifetime valid.
    read_tx: Option<Transaction<'static, RO, NoWriteMap>>,
    store: Arc<MdbxStore>,
    write_batch: WriteBatch,
}

impl std::fmt::Debug for MdbxSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MdbxSnapshot").finish_non_exhaustive()
    }
}

impl MdbxSnapshot {
    pub(crate) fn new(store: Arc<MdbxStore>) -> Self {
        let read_tx = store
            .read_txn()
            .map_err(|err| {
                warn!(target: "neo", error = %err, "MDBX snapshot open failed");
            })
            .ok();
        Self {
            read_tx,
            store,
            write_batch: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    fn read_entry(&self, key: &[u8]) -> crate::StorageResult<Option<Vec<u8>>> {
        let Some(read_tx) = self.read_tx.as_ref() else {
            return Ok(None);
        };
        let table = read_tx.open_table(None).map_err(super::store::mdbx_error)?;
        read_tx
            .get::<Vec<u8>>(&table, key)
            .map_err(super::store::mdbx_error)
    }

    fn collect_entries(
        &self,
        key_prefix: Option<&[u8]>,
        direction: SeekDirection,
    ) -> crate::StorageResult<Vec<(Vec<u8>, Vec<u8>)>> {
        let Some(read_tx) = self.read_tx.as_ref() else {
            return Ok(Vec::new());
        };
        let table = read_tx.open_table(None).map_err(super::store::mdbx_error)?;
        let mut cursor = read_tx.cursor(&table).map_err(super::store::mdbx_error)?;
        collect_cursor_entries(&mut cursor, key_prefix, direction)
    }
}

impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for MdbxSnapshot {
    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        match self.read_entry(key) {
            Ok(value) => value,
            Err(err) => {
                error!(target: "neo", error = %err, "MDBX snapshot get failed - this is a critical error that may cause incorrect state");
                #[cfg(debug_assertions)]
                panic!(
                    "MDBX snapshot storage read failed: {err}. This indicates a disk I/O error, corruption, or configuration problem that must be fixed before the node can operate correctly."
                );
                #[cfg(not(debug_assertions))]
                return None;
            }
        }
    }

    fn find(
        &self,
        key_prefix: Option<&Vec<u8>>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        match self.collect_entries(key_prefix.map(Vec::as_slice), direction) {
            Ok(entries) => Box::new(entries.into_iter()),
            Err(err) => {
                error!(target: "neo", error = %err, "MDBX snapshot find failed - this may cause incorrect state");
                Box::new(std::iter::empty())
            }
        }
    }
}

impl RawReadOnlyStore for MdbxSnapshot {
    fn try_get_bytes(&self, key: &[u8]) -> Option<Vec<u8>> {
        match self.read_entry(key) {
            Ok(value) => value,
            Err(err) => {
                error!(target: "neo", error = %err, "MDBX snapshot get failed - this is a critical error that may cause incorrect state");
                #[cfg(debug_assertions)]
                panic!(
                    "MDBX snapshot storage read failed: {err}. This indicates a disk I/O error, corruption, or configuration problem that must be fixed before the node can operate correctly."
                );
                #[cfg(not(debug_assertions))]
                return None;
            }
        }
    }
}

impl WriteStore<Vec<u8>, Vec<u8>> for MdbxSnapshot {
    fn delete(&mut self, key: Vec<u8>) -> crate::StorageResult<()> {
        self.write_batch.write().insert(key, None);
        Ok(())
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> crate::StorageResult<()> {
        self.write_batch.write().insert(key, Some(value));
        Ok(())
    }
}

impl StoreSnapshot for MdbxSnapshot {
    fn store(&self) -> Arc<dyn Store> {
        self.store.clone()
    }

    fn try_commit(&mut self) -> SnapshotCommitResult {
        {
            let batch = self.write_batch.read();
            self.store.commit_overlay(&batch)?;
        }
        self.write_batch.write().clear();
        Ok(())
    }
}
