//! Durable backend kind, diagnostics, and persistence dispatch.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use neo_storage::persistence::providers::RuntimeStore;

use crate::error::IndexerResult;
use crate::store;

use super::persistence::{MutationPersistenceMode, PendingPersistence, write_snapshot};

pub(super) enum PersistenceBackend {
    JsonFile(PathBuf),
    Store {
        store: Arc<RuntimeStore>,
        path: Option<PathBuf>,
    },
}

impl PersistenceBackend {
    pub(super) fn json_file(path: PathBuf) -> Self {
        Self::JsonFile(path)
    }

    pub(super) fn store(store: Arc<RuntimeStore>, path: Option<PathBuf>) -> Self {
        Self::Store { store, path }
    }

    pub(super) const fn mode_name(&self) -> &'static str {
        match self {
            Self::JsonFile(_) => "json-snapshot",
            Self::Store { .. } => "service-store",
        }
    }

    pub(super) fn snapshot_path(&self) -> Option<&Path> {
        match self {
            Self::JsonFile(path) => Some(path.as_path()),
            Self::Store { .. } => None,
        }
    }

    pub(super) fn store_path(&self) -> Option<&Path> {
        match self {
            Self::Store {
                path: Some(path), ..
            } => Some(path.as_path()),
            _ => None,
        }
    }

    pub(super) const fn mutation_mode(&self) -> MutationPersistenceMode {
        match self {
            Self::JsonFile(_) => MutationPersistenceMode::JsonFile,
            Self::Store { .. } => MutationPersistenceMode::Store,
        }
    }

    pub(super) fn store_backend(&self) -> Option<Arc<RuntimeStore>> {
        match self {
            Self::Store { store, .. } => Some(Arc::clone(store)),
            Self::JsonFile(_) => None,
        }
    }

    pub(super) fn persist_change(&self, change: PendingPersistence) -> IndexerResult<()> {
        match (self, change) {
            (Self::JsonFile(path), PendingPersistence::JsonSnapshot(snapshot)) => {
                write_snapshot(path, &snapshot)
            }
            (Self::Store { store, .. }, PendingPersistence::StoreDelta { previous, current }) => {
                store::write_indexer_delta(store, &previous, &current)
            }
            _ => Ok(()),
        }
    }
}
