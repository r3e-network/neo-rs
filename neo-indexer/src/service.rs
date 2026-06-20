//! Thread-safe service facade for the Neo indexer.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use neo_payloads::{ApplicationExecuted, Block};
use neo_primitives::UInt256;
use neo_storage::persistence::Store;
use parking_lot::{Mutex, RwLock};

use crate::error::IndexerResult;
use crate::indexer::Indexer;
use crate::model::{BlockIndexRecord, IndexerSnapshot, NotificationIndexRecord};
use crate::store;

mod notification_queries;
mod persistence;
mod query;

#[cfg(test)]
use persistence::temporary_snapshot_path;
use persistence::{MutationPersistenceMode, PendingPersistence, read_snapshot, write_snapshot};

/// Shared indexer service registered in `neo_system::ServiceRegistry`.
#[derive(Clone)]
pub struct IndexerService {
    inner: Arc<RwLock<Indexer>>,
    persist_lock: Arc<Mutex<()>>,
    persistence: Option<Arc<PersistenceBackend>>,
}

impl std::fmt::Debug for IndexerService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IndexerService")
            .field("status", &self.status())
            .field("persistence_mode", &self.persistence_mode())
            .field("snapshot_path", &self.snapshot_path())
            .field("store_path", &self.store_path())
            .finish_non_exhaustive()
    }
}

enum PersistenceBackend {
    JsonFile(PathBuf),
    Store {
        store: Arc<dyn Store>,
        path: Option<PathBuf>,
    },
}

impl Default for IndexerService {
    fn default() -> Self {
        Self::new()
    }
}

impl IndexerService {
    /// Creates an empty indexer service.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(Indexer::new())),
            persist_lock: Arc::new(Mutex::new(())),
            persistence: None,
        }
    }

    /// Opens a persistent indexer service backed by a JSON snapshot file.
    ///
    /// If the snapshot file is absent, the service starts empty and creates the
    /// file on the first mutation.
    pub fn open(path: impl AsRef<Path>) -> IndexerResult<Self> {
        let path = path.as_ref().to_path_buf();
        let indexer = read_snapshot(&path)?;
        Ok(Self {
            inner: Arc::new(RwLock::new(indexer)),
            persist_lock: Arc::new(Mutex::new(())),
            persistence: Some(Arc::new(PersistenceBackend::JsonFile(path))),
        })
    }

    /// Opens a persistent indexer service backed by a generic service store.
    pub fn open_store(store: Arc<dyn Store>) -> IndexerResult<Self> {
        Self::open_store_with_path(store, None::<PathBuf>)
    }

    /// Opens a persistent indexer service backed by a generic service store and
    /// records the operator-facing store path for diagnostics.
    pub fn open_store_with_path(
        store: Arc<dyn Store>,
        path: Option<impl Into<PathBuf>>,
    ) -> IndexerResult<Self> {
        let indexer = store::read_indexer(&store)?;
        Ok(Self {
            inner: Arc::new(RwLock::new(indexer)),
            persist_lock: Arc::new(Mutex::new(())),
            persistence: Some(Arc::new(PersistenceBackend::Store {
                store,
                path: path.map(Into::into),
            })),
        })
    }

    /// Returns whether this service has a durable persistence backend.
    pub fn is_persistent(&self) -> bool {
        self.persistence.is_some()
    }

    /// Returns a stable name for the configured persistence backend.
    pub fn persistence_mode(&self) -> &'static str {
        match self.persistence.as_deref() {
            None => "memory",
            Some(PersistenceBackend::JsonFile(_)) => "json-snapshot",
            Some(PersistenceBackend::Store { .. }) => "service-store",
        }
    }

    /// Returns the persistent JSON snapshot path, if this service was opened
    /// with one.
    pub fn snapshot_path(&self) -> Option<&Path> {
        match self.persistence.as_deref() {
            Some(PersistenceBackend::JsonFile(path)) => Some(path.as_path()),
            _ => None,
        }
    }

    /// Returns the persistent service-store path, if one was supplied.
    pub fn store_path(&self) -> Option<&Path> {
        match self.persistence.as_deref() {
            Some(PersistenceBackend::Store {
                path: Some(path), ..
            }) => Some(path.as_path()),
            _ => None,
        }
    }

    /// Indexes a canonical block.
    pub fn index_block(&self, block: &Block) -> IndexerResult<BlockIndexRecord> {
        self.mutate_indexer(|indexer| {
            let record = indexer.index_block(block)?;
            Ok((record, true))
        })
    }

    /// Indexes a canonical block and its emitted smart-contract notifications.
    pub fn index_block_with_application_executions(
        &self,
        block: &Block,
        executions: &[ApplicationExecuted],
    ) -> IndexerResult<BlockIndexRecord> {
        self.mutate_indexer(|indexer| {
            let record = indexer.index_block_with_application_executions(block, executions)?;
            Ok((record, true))
        })
    }

    /// Indexes a canonical block with notification records recovered from
    /// durable plugin data.
    pub fn index_block_with_notification_records(
        &self,
        block: &Block,
        notifications: Vec<NotificationIndexRecord>,
    ) -> IndexerResult<BlockIndexRecord> {
        self.mutate_indexer(|indexer| {
            let record = indexer.index_block_with_notification_records(block, notifications)?;
            Ok((record, true))
        })
    }

    /// Removes an indexed block by hash.
    pub fn remove_block_by_hash(&self, hash: &UInt256) -> IndexerResult<Option<BlockIndexRecord>> {
        self.mutate_indexer(|indexer| {
            let removed = indexer.remove_block_by_hash(hash);
            let should_persist = removed.is_some();
            Ok((removed, should_persist))
        })
    }

    /// Removes an indexed block by height.
    pub fn remove_block_at_height(&self, height: u32) -> IndexerResult<Option<BlockIndexRecord>> {
        self.mutate_indexer(|indexer| {
            let removed = indexer.remove_block_at_height(height);
            let should_persist = removed.is_some();
            Ok((removed, should_persist))
        })
    }

    /// Removes all indexed blocks above `height`.
    pub fn revert_to_height(&self, height: u32) -> IndexerResult<Vec<BlockIndexRecord>> {
        self.mutate_indexer(|indexer| {
            let removed = indexer.revert_to_height(height);
            let should_persist = !removed.is_empty();
            Ok((removed, should_persist))
        })
    }

    fn persistence_guard(&self) -> Option<parking_lot::MutexGuard<'_, ()>> {
        self.persistence.as_ref().map(|_| self.persist_lock.lock())
    }

    fn snapshot_for_persistence(&self, indexer: &Indexer) -> Option<IndexerSnapshot> {
        self.persistence.as_ref().map(|_| indexer.snapshot())
    }

    fn persistence_mode_for_mutation(&self) -> MutationPersistenceMode {
        match self.persistence.as_deref() {
            Some(PersistenceBackend::JsonFile(_)) => MutationPersistenceMode::JsonFile,
            Some(PersistenceBackend::Store { .. }) => MutationPersistenceMode::Store,
            None => MutationPersistenceMode::None,
        }
    }

    fn mutate_indexer<T>(
        &self,
        mutate: impl FnOnce(&mut Indexer) -> IndexerResult<(T, bool)>,
    ) -> IndexerResult<T> {
        let _persist_guard = self.persistence_guard();
        let mode = self.persistence_mode_for_mutation();
        let (result, change, rollback_snapshot) = {
            let mut indexer = self.inner.write();
            let rollback_snapshot = if mode.is_persistent() {
                Some(indexer.snapshot())
            } else {
                None
            };
            let (result, should_persist) = mutate(&mut indexer)?;
            let change = if should_persist {
                match mode {
                    MutationPersistenceMode::None => None,
                    MutationPersistenceMode::JsonFile => self
                        .snapshot_for_persistence(&indexer)
                        .map(PendingPersistence::JsonSnapshot),
                    MutationPersistenceMode::Store => {
                        let previous = rollback_snapshot.clone().ok_or(
                            crate::IndexerError::MissingRollbackSnapshot {
                                mode: "service-store",
                            },
                        )?;
                        Some(PendingPersistence::StoreDelta {
                            previous,
                            current: indexer.snapshot(),
                        })
                    }
                }
            } else {
                None
            };
            (result, change, rollback_snapshot)
        };
        if let Err(err) = self.persist_change(change) {
            if let Some(snapshot) = rollback_snapshot {
                self.restore_indexer_after_persistence_failure(snapshot);
            }
            return Err(err);
        }
        Ok(result)
    }

    fn persist_change(&self, change: Option<PendingPersistence>) -> IndexerResult<()> {
        match (self.persistence.as_deref(), change) {
            (
                Some(PersistenceBackend::JsonFile(path)),
                Some(PendingPersistence::JsonSnapshot(snapshot)),
            ) => write_snapshot(path, &snapshot),
            (
                Some(PersistenceBackend::Store { store, .. }),
                Some(PendingPersistence::StoreDelta { previous, current }),
            ) => store::write_indexer_delta(store, &previous, &current),
            _ => Ok(()),
        }
    }

    fn restore_indexer_after_persistence_failure(&self, snapshot: IndexerSnapshot) {
        match Indexer::from_snapshot(snapshot) {
            Ok(indexer) => {
                *self.inner.write() = indexer;
            }
            Err(err) => {
                tracing::error!(
                    target: "neo::indexer",
                    error = %err,
                    "failed to roll back in-memory indexer after persistence failure"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests;
