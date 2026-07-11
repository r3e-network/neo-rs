//! Startup reconciliation from the authoritative hot Ledger prefix.

use neo_error::{CoreError, CoreResult};
use neo_storage::{CacheRead, DataCache};

use crate::ledger::ledger_provider::{BlockProvider, StorageLedgerProvider};

use super::StaticLedgerArchive;

/// Outcome of reconciling a static archive to the durable hot Ledger tip.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StaticArchiveRecovery {
    /// Number of archive-tail blocks removed because they exceeded hot truth.
    pub truncated_blocks: u32,
    /// Number of durable hot blocks appended to repair archive lag.
    pub appended_blocks: u32,
    /// Archive tip after reconciliation.
    pub final_tip: Option<u32>,
}

impl StaticLedgerArchive {
    /// Reconciles the archive against the authoritative canonical store.
    ///
    /// Static files are a post-canonical mirror in this phase: an incomplete
    /// tail is repaired by the static-file opener, then every overlapping block
    /// hash is checked against hot truth before an ahead tail is truncated or
    /// missing finalized blocks are replayed in bounded batches. Hot data
    /// remains authoritative until a later, separately verified pruning phase
    /// is enabled.
    pub fn reconcile<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        canonical_tip: Option<u32>,
        batch_size: usize,
    ) -> CoreResult<StaticArchiveRecovery> {
        if batch_size == 0 {
            return Err(CoreError::invalid_operation(
                "static archive recovery batch size must be greater than zero",
            ));
        }
        let original_tip = self.tip();
        let Some(canonical_tip) = canonical_tip else {
            self.truncate_after(None)?;
            return Ok(StaticArchiveRecovery {
                truncated_blocks: original_tip.map_or(0, |height| height.saturating_add(1)),
                appended_blocks: 0,
                final_tip: None,
            });
        };

        let hot = StorageLedgerProvider::new(snapshot);
        if let Some(archive_tip) = original_tip {
            let archived = self.provider();
            for height in 0..=archive_tip.min(canonical_tip) {
                let archived_hash = archived.block_hash_by_index(height)?.ok_or_else(|| {
                    CoreError::invalid_data(format!(
                        "static archive has no block-hash row at height {height}"
                    ))
                })?;
                let hot_hash = hot.block_hash_by_index(height)?.ok_or_else(|| {
                    CoreError::invalid_data(format!(
                        "hot Ledger has no block hash at archived height {height}"
                    ))
                })?;
                if archived_hash != hot_hash {
                    return Err(CoreError::invalid_data(format!(
                        "static archive fork mismatch at height {height}: archive={archived_hash}, hot={hot_hash}"
                    )));
                }
            }
        }

        if original_tip.is_some_and(|tip| tip > canonical_tip) {
            self.truncate_after(Some(canonical_tip))?;
        }
        let truncated_blocks = original_tip
            .map(|tip| tip.saturating_sub(canonical_tip))
            .unwrap_or(0);

        let mut next_height = match self.tip() {
            Some(tip) => tip.checked_add(1),
            None => Some(0),
        };
        let mut appended_blocks = 0u32;
        while let Some(batch_start) = next_height.filter(|height| *height <= canonical_tip) {
            let remaining = u64::from(canonical_tip) - u64::from(batch_start) + 1;
            let count = usize::try_from(remaining)
                .unwrap_or(usize::MAX)
                .min(batch_size);
            let mut records = Vec::with_capacity(count);
            for offset in 0..count {
                let height = batch_start
                    .checked_add(u32::try_from(offset).map_err(|_| {
                        CoreError::invalid_operation("archive recovery offset exceeds u32")
                    })?)
                    .ok_or_else(|| {
                        CoreError::invalid_operation("archive recovery height overflow")
                    })?;
                let block = hot.block_by_index(height)?.ok_or_else(|| {
                    CoreError::invalid_data(format!(
                        "hot Ledger block {height} is missing during archive recovery"
                    ))
                })?;
                records.push(self.capture_block(snapshot, &block)?);
            }
            self.append_records(records)?;
            let count_u32 = u32::try_from(count)
                .map_err(|_| CoreError::invalid_operation("archive recovery batch exceeds u32"))?;
            appended_blocks = appended_blocks.saturating_add(count_u32);
            next_height = batch_start.checked_add(count_u32);
        }

        Ok(StaticArchiveRecovery {
            truncated_blocks,
            appended_blocks,
            final_tip: self.tip(),
        })
    }
}
