//! Benchmark adapter over the extracted `neo-state-packs` engine.
//!
//! The append-frame engine (frames, index runs, manifests, compaction,
//! leases, GC) lives in the `neo-state-packs` workspace crate. This shim
//! keeps the benchmark's `WorkloadOperation` vocabulary and measured
//! `AppendStore` call shape without duplicating the engine.

use super::AppendStageTotals;
use crate::storage_workload::{MPT_NODE_KEY_BYTES, OperationKind, WorkloadOperation};
use anyhow::Result;
use neo_state_packs::{
    CompactionStats, GcStats, OpenValidation, PackFrameBuilder, PackStore, PackStoreConfig,
};
use std::path::Path;

pub(super) struct AppendStore {
    inner: PackStore,
}

impl AppendStore {
    pub(super) fn create(root: &Path, max_index_memory_bytes: u64) -> Result<Self> {
        let config =
            PackStoreConfig::default().with_max_index_memory_bytes(max_index_memory_bytes)?;
        Ok(Self {
            inner: PackStore::create(root, config)?,
        })
    }

    pub(super) fn open(root: &Path, max_index_memory_bytes: u64) -> Result<Self> {
        let config =
            PackStoreConfig::default().with_max_index_memory_bytes(max_index_memory_bytes)?;
        Ok(Self {
            inner: PackStore::open(root, config)?,
        })
    }

    pub(super) fn append(&mut self, operations: &[WorkloadOperation]) -> Result<AppendStageTotals> {
        let value_bytes = operations.iter().try_fold(0u64, |total, operation| {
            let value_bytes = match &operation.kind {
                OperationKind::Put(value) => u64::try_from(value.len())?,
                OperationKind::Tombstone => 0,
            };
            total
                .checked_add(value_bytes)
                .ok_or_else(|| anyhow::anyhow!("append workload value bytes overflow u64"))
        })?;
        let mut builder = PackFrameBuilder::with_value_bytes(operations.len(), value_bytes)?;
        for operation in operations {
            let value = match &operation.kind {
                OperationKind::Put(value) => Some(value.as_slice()),
                OperationKind::Tombstone => None,
            };
            builder.push_key(operation.key, value)?;
        }
        self.inner.append_built(builder)
    }

    pub(super) fn get(&self, key: &[u8; MPT_NODE_KEY_BYTES]) -> Result<Option<Vec<u8>>> {
        self.inner.get(key)
    }

    pub(super) fn get_many_sorted(
        &self,
        keys: &[[u8; MPT_NODE_KEY_BYTES]],
    ) -> Result<Vec<Option<Vec<u8>>>> {
        self.inner.get_many_sorted(keys)
    }

    pub(super) fn gc(&mut self) -> Result<GcStats> {
        self.inner.gc()
    }

    pub(super) fn layout(&self) -> Result<(u64, u64, u64, u64)> {
        self.inner.layout()
    }

    pub(super) const fn open_validation(&self) -> OpenValidation {
        self.inner.open_validation()
    }

    pub(super) const fn compaction_stats(&self) -> CompactionStats {
        self.inner.compaction_stats()
    }
}
