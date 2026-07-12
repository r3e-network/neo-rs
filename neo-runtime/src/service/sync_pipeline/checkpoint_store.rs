//! Store-backed sync-stage checkpoint adapters.
//!
//! Versioned checkpoint records live in a typed table inside backend-isolated
//! maintenance metadata. The canonical chain tip remains authoritative and
//! production sync realigns its cursor to that tip.

use std::sync::Arc;

use neo_storage::persistence::providers::memory_store::MemoryStore;
use neo_storage::persistence::{StoreMaintenanceBatch, TableProvider, TransactionalStore};

use super::tables::SyncCheckpointTable;
use super::{SyncStageCheckpoint, SyncStageCheckpointStore, SyncStageKind};
use crate::{ServiceError, ServiceResult};

/// Store-backed checkpoint provider for crash-resumable sync stages.
///
/// Checkpoint bytes live in the backend's isolated maintenance namespace and
/// therefore cannot enter Neo contract scans, store dumps, or state roots.
#[derive(Debug)]
pub struct StoreSyncStageCheckpointStore<S: TransactionalStore> {
    store: S,
}

impl<S: TransactionalStore> StoreSyncStageCheckpointStore<S> {
    /// Creates a checkpoint store over a concrete storage backend handle.
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    /// Returns the underlying store handle.
    #[must_use]
    pub const fn store(&self) -> &S {
        &self.store
    }
}

impl<S: TransactionalStore> SyncStageCheckpointStore for StoreSyncStageCheckpointStore<S> {
    fn checkpoint(&self, stage: SyncStageKind) -> ServiceResult<Option<SyncStageCheckpoint>> {
        read_checkpoint(&self.store, stage)
    }

    fn put_checkpoint(&self, checkpoint: SyncStageCheckpoint) -> ServiceResult<()> {
        write_checkpoint(&self.store, checkpoint)
    }
}

/// Store-backed checkpoint provider over a shared store handle.
///
/// Node composition carries `Arc<S>` across RPC, blockchain, and sync
/// services. This adapter keeps the concrete backend type and uses the same
/// maintenance transaction as the owned adapter.
#[derive(Debug)]
pub struct SharedStoreSyncStageCheckpointStore<S: TransactionalStore = MemoryStore> {
    store: Arc<S>,
}

impl<S: TransactionalStore> SharedStoreSyncStageCheckpointStore<S> {
    /// Creates a checkpoint store over a shared storage backend.
    #[must_use]
    pub const fn new(store: Arc<S>) -> Self {
        Self { store }
    }

    /// Returns the shared store handle.
    #[must_use]
    pub fn store(&self) -> Arc<S> {
        Arc::clone(&self.store)
    }
}

impl<S: TransactionalStore> SyncStageCheckpointStore for SharedStoreSyncStageCheckpointStore<S> {
    fn checkpoint(&self, stage: SyncStageKind) -> ServiceResult<Option<SyncStageCheckpoint>> {
        read_checkpoint(self.store.as_ref(), stage)
    }

    fn put_checkpoint(&self, checkpoint: SyncStageCheckpoint) -> ServiceResult<()> {
        write_checkpoint(self.store.as_ref(), checkpoint)
    }
}

pub(crate) fn read_checkpoint<S: TransactionalStore>(
    store: &S,
    stage: SyncStageKind,
) -> ServiceResult<Option<SyncStageCheckpoint>> {
    let checkpoint = store
        .table_get::<SyncCheckpointTable>(&stage)
        .map_err(|error| table_read_error("read sync checkpoint", error))?;
    if let Some(checkpoint) = &checkpoint
        && checkpoint.stage != stage
    {
        return Err(ServiceError::invalid_state(format!(
            "sync checkpoint stage mismatch: requested {}, stored {}",
            stage.as_str(),
            checkpoint.stage.as_str()
        )));
    }
    Ok(checkpoint)
}

pub(crate) fn write_checkpoint<S: TransactionalStore>(
    store: &S,
    checkpoint: SyncStageCheckpoint,
) -> ServiceResult<()> {
    let mut maintenance = StoreMaintenanceBatch::new();
    maintenance
        .put::<SyncCheckpointTable>(&checkpoint.stage, &checkpoint)
        .map_err(|error| storage_error("encode sync checkpoint", error))?;
    commit_maintenance(store, &maintenance, "write sync checkpoint")
}

pub(crate) fn commit_maintenance<S: TransactionalStore>(
    store: &S,
    maintenance: &StoreMaintenanceBatch,
    operation: &'static str,
) -> ServiceResult<()> {
    store
        .commit_maintenance(maintenance)
        .map_err(|error| storage_error(operation, error))?;
    Ok(())
}

pub(crate) fn storage_error(context: &'static str, error: impl std::fmt::Display) -> ServiceError {
    ServiceError::internal(format!("{context}: {error}"))
}

pub(crate) fn table_read_error(
    context: &'static str,
    error: neo_storage::StorageError,
) -> ServiceError {
    match error {
        neo_storage::StorageError::Serialization { .. } => {
            ServiceError::invalid_state(format!("{context}: {error}"))
        }
        _ => storage_error(context, error),
    }
}
