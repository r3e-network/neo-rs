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
    CompactionStats, GcStats, OpenValidation, PackOpKind, PackOperation, PackStore,
};
use std::path::Path;

pub(super) struct AppendStore {
    inner: PackStore,
}

impl AppendStore {
    pub(super) fn create(root: &Path, max_index_memory_bytes: u64) -> Result<Self> {
        Ok(Self {
            inner: PackStore::create(root, max_index_memory_bytes)?,
        })
    }

    pub(super) fn open(root: &Path, max_index_memory_bytes: u64) -> Result<Self> {
        Ok(Self {
            inner: PackStore::open(root, max_index_memory_bytes)?,
        })
    }

    pub(super) fn append(&mut self, operations: &[WorkloadOperation]) -> Result<AppendStageTotals> {
        let operations: Vec<PackOperation> = operations
            .iter()
            .map(|operation| PackOperation {
                key: operation.key,
                kind: match &operation.kind {
                    OperationKind::Put(value) => PackOpKind::Put(value.clone()),
                    OperationKind::Tombstone => PackOpKind::Tombstone,
                },
            })
            .collect();
        self.inner.append(&operations)
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
