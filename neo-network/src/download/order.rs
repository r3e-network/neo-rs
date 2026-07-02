//! Ordered block batch buffering.
//!
//! Multi-peer downloads complete in network order, but the import pipeline must
//! receive contiguous block heights. `OrderedBlockBatchBuffer` is the small
//! policy object that validates remote response shape, holds out-of-order
//! batches, and releases only the next expected height.

use std::collections::BTreeMap;

use super::BlockDownloadBatch;
use crate::{NetworkError, NetworkResult};

/// Ordered buffer for downloaded block batches.
#[derive(Clone, Debug, Default)]
pub struct OrderedBlockBatchBuffer {
    next_height: u32,
    pending: BTreeMap<u32, BlockDownloadBatch>,
}

impl OrderedBlockBatchBuffer {
    /// Construct a buffer that expects `next_height` first.
    #[must_use]
    pub const fn new(next_height: u32) -> Self {
        Self {
            next_height,
            pending: BTreeMap::new(),
        }
    }

    /// Height required for the next emitted batch.
    #[must_use]
    pub const fn next_height(&self) -> u32 {
        self.next_height
    }

    /// Number of buffered batches waiting behind a gap.
    #[must_use]
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// Returns `true` when no out-of-order batch is buffered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Insert a downloaded batch after validating its height sequence.
    pub fn insert(&mut self, batch: BlockDownloadBatch) -> NetworkResult<()> {
        self.validate_batch(&batch)?;
        if batch.start_height < self.next_height {
            return Err(NetworkError::Protocol(format!(
                "stale block download batch starts at {}, expected at least {}",
                batch.start_height, self.next_height
            )));
        }
        if self.pending.contains_key(&batch.start_height) {
            return Err(NetworkError::Protocol(format!(
                "duplicate block download batch starts at {}",
                batch.start_height
            )));
        }
        self.pending.insert(batch.start_height, batch);
        Ok(())
    }

    /// Pop the next contiguous batch, if available.
    pub fn pop_ready(&mut self) -> Option<BlockDownloadBatch> {
        let batch = self.pending.remove(&self.next_height)?;
        self.next_height = batch.next_height();
        Some(batch)
    }

    fn validate_batch(&self, batch: &BlockDownloadBatch) -> NetworkResult<()> {
        if batch.is_empty() {
            return Err(NetworkError::Protocol(
                "empty block download batch cannot advance sync".to_string(),
            ));
        }
        for (offset, block) in batch.blocks.iter().enumerate() {
            let expected = batch
                .start_height
                .saturating_add(u32::try_from(offset).unwrap_or(u32::MAX));
            if block.index() != expected {
                return Err(NetworkError::Protocol(format!(
                    "block download batch at {} contains block {} at offset {}",
                    batch.start_height,
                    block.index(),
                    offset
                )));
            }
        }
        Ok(())
    }
}
