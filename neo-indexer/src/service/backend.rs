//! Durable backend kind, diagnostics, and persistence dispatch.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use neo_storage::persistence::{Store, providers::RuntimeStore};

use crate::error::IndexerResult;
use crate::store;

pub(super) struct PersistenceBackend {
    store: Arc<RuntimeStore>,
    path: Option<PathBuf>,
}

impl PersistenceBackend {
    pub(super) fn store(store: Arc<RuntimeStore>, path: Option<PathBuf>) -> Self {
        Self { store, path }
    }

    pub(super) const fn mode_name(&self) -> &'static str {
        "service-store"
    }

    pub(super) fn store_path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub(super) fn store_backend(&self) -> Option<Arc<RuntimeStore>> {
        Some(Arc::clone(&self.store))
    }

    pub(super) fn persist_change(
        &self,
        previous: &crate::model::IndexerSnapshot,
        current: &crate::model::IndexerSnapshot,
    ) -> IndexerResult<()> {
        store::write_indexer_delta(&self.store, previous, current)
    }

    pub(super) fn persist_projection_change(
        &self,
        change: &crate::indexer::ProjectionChangeSet,
    ) -> IndexerResult<()> {
        store::write_indexer_change_set(&self.store, change)
    }

    pub(super) fn flush_durable(&self) -> IndexerResult<()> {
        self.store
            .flush()
            .map_err(|source| crate::IndexerError::StoreRecordWrite { source })
    }
}
