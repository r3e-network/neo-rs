//! Store-backed sync-stage checkpoint adapters.
//!
//! Versioned checkpoint records live in backend-isolated maintenance metadata.
//! Obsolete short-key records from the normal Neo data table are discarded,
//! not migrated: they are operational hints, while the canonical chain tip is
//! authoritative and production sync realigns its cursor to that tip.

use std::sync::Arc;

use neo_storage::persistence::providers::memory_store::MemoryStore;
use neo_storage::persistence::{Store, StoreMaintenanceBatch};

use super::{
    SyncStageCheckpoint, SyncStageCheckpointStore, SyncStageKind, checkpoint_key,
    decode_checkpoint, encode_checkpoint, legacy_checkpoint_key,
};
use crate::{ServiceError, ServiceResult};

/// Store-backed checkpoint provider for crash-resumable sync stages.
///
/// Checkpoint bytes live in the backend's isolated maintenance namespace and
/// therefore cannot enter Neo contract scans, store dumps, or state roots.
#[derive(Debug)]
pub struct StoreSyncStageCheckpointStore<S: Store> {
    store: S,
}

impl<S: Store> StoreSyncStageCheckpointStore<S> {
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

impl<S: Store> SyncStageCheckpointStore for StoreSyncStageCheckpointStore<S> {
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
pub struct SharedStoreSyncStageCheckpointStore<S: Store = MemoryStore> {
    store: Arc<S>,
}

impl<S: Store> SharedStoreSyncStageCheckpointStore<S> {
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

impl<S: Store> SyncStageCheckpointStore for SharedStoreSyncStageCheckpointStore<S> {
    fn checkpoint(&self, stage: SyncStageKind) -> ServiceResult<Option<SyncStageCheckpoint>> {
        read_checkpoint(self.store.as_ref(), stage)
    }

    fn put_checkpoint(&self, checkpoint: SyncStageCheckpoint) -> ServiceResult<()> {
        write_checkpoint(self.store.as_ref(), checkpoint)
    }
}

fn read_checkpoint<S: Store>(
    store: &S,
    stage: SyncStageKind,
) -> ServiceResult<Option<SyncStageCheckpoint>> {
    discard_legacy_checkpoint(store, stage)?;
    let Some(bytes) = store
        .maintenance_metadata(&checkpoint_key(stage))
        .map_err(|error| storage_error("read sync checkpoint", error))?
    else {
        return Ok(None);
    };
    decode_checkpoint(stage, &bytes).map(Some)
}

fn write_checkpoint<S: Store>(store: &S, checkpoint: SyncStageCheckpoint) -> ServiceResult<()> {
    let mut maintenance = StoreMaintenanceBatch::new();
    maintenance.delete_data(legacy_checkpoint_key(checkpoint.stage));
    maintenance.put_metadata(
        checkpoint_key(checkpoint.stage),
        encode_checkpoint(&checkpoint),
    );
    commit_maintenance(store, &maintenance, "write sync checkpoint")
}

fn discard_legacy_checkpoint<S: Store>(store: &S, stage: SyncStageKind) -> ServiceResult<()> {
    let legacy_key = legacy_checkpoint_key(stage);
    if store.try_get_bytes(&legacy_key).is_none() {
        return Ok(());
    }
    let mut maintenance = StoreMaintenanceBatch::new();
    maintenance.delete_data(legacy_key);
    commit_maintenance(store, &maintenance, "discard legacy sync checkpoint")
}

fn commit_maintenance<S: Store>(
    store: &S,
    maintenance: &StoreMaintenanceBatch,
    operation: &'static str,
) -> ServiceResult<()> {
    if !store
        .try_commit_durable_maintenance(maintenance)
        .map_err(|error| storage_error(operation, error))?
    {
        return Err(ServiceError::invalid_state(
            "store does not support atomic sync-checkpoint maintenance",
        ));
    }
    Ok(())
}

fn storage_error(context: &'static str, error: impl std::fmt::Display) -> ServiceError {
    ServiceError::internal(format!("{context}: {error}"))
}
