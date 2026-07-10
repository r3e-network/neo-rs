//! # neo-indexer::service
//!
//! Service loops, handles, lifecycle helpers, and command processing.
//!
//! ## Boundary
//!
//! This module belongs to `neo-indexer`. This service crate owns projections
//! over committed chain data and must not decide block validity or consensus
//! outcomes.
//!
//! ## Contents
//!
//! - `backend`: durable backend kind, diagnostics, and persistence dispatch.
//! - `commands`: public indexing and revert commands.
//! - `mutation`: persistence-aware mutation and rollback mechanics.
//! - `notification_queries`: notification query API.
//! - `persistence`: Persistence traits, snapshots, transactions, and cache
//!   overlays.
//! - `query`: query APIs for indexed data.
//! - `tests`: Module-local tests and regression coverage.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use neo_storage::persistence::{Store, providers::RuntimeStore};
use parking_lot::{Mutex, RwLock};

use crate::error::IndexerResult;
use crate::indexer::Indexer;
use crate::store;

mod backend;
mod commands;
mod mutation;
mod notification_queries;
mod persistence;
mod query;

use backend::PersistenceBackend;
use persistence::read_snapshot;
#[cfg(test)]
use persistence::temporary_snapshot_path;
#[cfg(test)]
use persistence::write_snapshot;

/// Shared indexer service installed in the node's typed RPC service bundle.
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
            persistence: Some(Arc::new(PersistenceBackend::json_file(path))),
        })
    }

    /// Opens a persistent indexer service backed by a generic service store.
    pub fn open_store<S>(store: Arc<S>) -> IndexerResult<Self>
    where
        S: Store + Clone + Into<RuntimeStore> + 'static,
    {
        Self::open_store_with_path(store, None::<PathBuf>)
    }

    /// Opens a persistent indexer service backed by a generic service store and
    /// records the operator-facing store path for diagnostics.
    pub fn open_store_with_path<S>(
        store: Arc<S>,
        path: Option<impl Into<PathBuf>>,
    ) -> IndexerResult<Self>
    where
        S: Store + Clone + Into<RuntimeStore> + 'static,
    {
        let indexer = store::read_indexer(&store)?;
        let store = Arc::new(store.as_ref().clone().into());
        Ok(Self {
            inner: Arc::new(RwLock::new(indexer)),
            persist_lock: Arc::new(Mutex::new(())),
            persistence: Some(Arc::new(PersistenceBackend::store(
                store,
                path.map(Into::into),
            ))),
        })
    }

    /// Returns whether this service has a durable persistence backend.
    pub fn is_persistent(&self) -> bool {
        self.persistence.is_some()
    }

    /// Returns a stable name for the configured persistence backend.
    pub fn persistence_mode(&self) -> &'static str {
        self.persistence
            .as_deref()
            .map_or("memory", PersistenceBackend::mode_name)
    }

    /// Returns the persistent JSON snapshot path, if this service was opened
    /// with one.
    pub fn snapshot_path(&self) -> Option<&Path> {
        self.persistence
            .as_deref()
            .and_then(PersistenceBackend::snapshot_path)
    }

    /// Returns the persistent service-store path, if one was supplied.
    pub fn store_path(&self) -> Option<&Path> {
        self.persistence
            .as_deref()
            .and_then(PersistenceBackend::store_path)
    }
}

#[cfg(test)]
#[path = "../tests/service/mod.rs"]
mod tests;
