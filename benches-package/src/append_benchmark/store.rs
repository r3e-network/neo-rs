//! Benchmark adapter over the extracted `neo-state-packs` engine.
//!
//! The append-frame engine (frames, index runs, manifests, compaction,
//! leases, GC) lives in the `neo-state-packs` workspace crate. This shim
//! keeps the benchmark's `WorkloadOperation` vocabulary and measured
//! `AppendStore` call shape without duplicating the engine.

use super::AppendStageTotals;
use crate::storage_workload::{MPT_NODE_KEY_BYTES, OperationKind, WorkloadOperation};
use anyhow::{Result, ensure};
use neo_state_packs::{
    CompactionStats, GcStats, OpenValidation, PackFrameContext, PackStore, PackStoreConfig,
};
use std::path::Path;

pub(super) struct AppendStore {
    inner: PackStore,
    next_block: u32,
    previous_root: [u8; 32],
}

impl AppendStore {
    pub(super) fn create(root: &Path, max_index_memory_bytes: u64) -> Result<Self> {
        let config =
            PackStoreConfig::default().with_max_index_memory_bytes(max_index_memory_bytes)?;
        Ok(Self {
            inner: PackStore::create(root, config)?,
            next_block: 0,
            previous_root: [0; 32],
        })
    }

    pub(super) fn open(root: &Path, max_index_memory_bytes: u64) -> Result<Self> {
        let config =
            PackStoreConfig::default().with_max_index_memory_bytes(max_index_memory_bytes)?;
        let inner = PackStore::open(root, config)?;
        let (next_block, previous_root) = inner.last_frame_receipt().map_or_else(
            || Ok((0, [0; 32])),
            |receipt| -> Result<_> {
                let block = u32::try_from(receipt.epoch)
                    .map_err(|_| anyhow::anyhow!("synthetic frame epoch exceeds u32"))?;
                let expected_previous = block.checked_sub(1).map_or([0; 32], synthetic_root);
                ensure!(
                    receipt.context
                        == PackFrameContext::new(
                            block,
                            block,
                            expected_previous,
                            synthetic_root(block),
                        ),
                    "reopened synthetic frame context differs from its deterministic cursor"
                );
                Ok((
                    block
                        .checked_add(1)
                        .ok_or_else(|| anyhow::anyhow!("synthetic frame block cursor overflow"))?,
                    receipt.context.resulting_root,
                ))
            },
        )?;
        Ok(Self {
            inner,
            next_block,
            previous_root,
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
        let resulting_root = synthetic_root(self.next_block);
        let context = PackFrameContext::new(
            self.next_block,
            self.next_block,
            self.previous_root,
            resulting_root,
        );
        let mut builder =
            self.inner
                .frame_builder_with_value_bytes(context, operations.len(), value_bytes)?;
        let mut order = (0..operations.len()).collect::<Vec<_>>();
        order.sort_unstable_by(|left, right| {
            operations[*left]
                .key
                .cmp(&operations[*right].key)
                .then_with(|| left.cmp(right))
        });
        for index in order {
            let operation = &operations[index];
            let value = match &operation.kind {
                OperationKind::Put(value) => Some(value.as_slice()),
                OperationKind::Tombstone => None,
            };
            builder.push_key(operation.key, value)?;
        }
        let totals = self.inner.append_built(builder)?;
        self.next_block = self
            .next_block
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("synthetic frame block cursor overflow"))?;
        self.previous_root = resulting_root;
        Ok(totals)
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

fn synthetic_root(block: u32) -> [u8; 32] {
    let mut root = [0xA5; 32];
    root[0..4].copy_from_slice(&block.to_le_bytes());
    root[28..32].copy_from_slice(&block.wrapping_mul(0x9E37_79B9).to_be_bytes());
    root
}
