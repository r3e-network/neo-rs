//! Startup reconciliation across canonical hot suffix and archived prefix.

use neo_error::{CoreError, CoreResult};
use neo_storage::{CacheRead, DataCache};

use crate::ledger::ledger_provider::{BlockProvider, StorageLedgerProvider};

use super::StaticLedgerArchive;

/// Outcome of reconciling a static archive to the durable canonical Ledger tip.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StaticArchiveRecovery {
    /// Number of archive-tail blocks removed because they exceeded hot truth.
    pub truncated_blocks: u32,
    /// Number of durable hot blocks appended to repair archive lag.
    pub appended_blocks: u32,
    /// Archive tip after reconciliation.
    pub final_tip: Option<u32>,
    /// Existing hot-pruning watermark validated during reconciliation.
    pub hot_pruned_through: Option<u32>,
}

impl StaticLedgerArchive {
    /// Reconciles the archive against canonical storage and its prune watermark.
    ///
    /// Static-file bytes are durably fenced before the canonical hot
    /// transaction, while their provider-visible index is published only after
    /// hot success. The opener recovers a complete unpublished suffix, then
    /// reconciliation checks every still-hot overlapping block hash before
    /// truncating a cold-ahead suffix left by an interrupted canonical commit
    /// or replaying missing finalized blocks in bounded batches. Heights at or
    /// below `hot_pruned_through` are archive-authoritative and are never
    /// expected to remain in the hot store.
    pub fn reconcile<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        canonical_tip: Option<u32>,
        hot_pruned_through: Option<u32>,
        batch_size: usize,
    ) -> CoreResult<StaticArchiveRecovery> {
        if batch_size == 0 {
            return Err(CoreError::invalid_operation(
                "static archive recovery batch size must be greater than zero",
            ));
        }
        let original_tip = self.tip();
        if let Some(watermark) = hot_pruned_through {
            let canonical_tip = canonical_tip.ok_or_else(|| {
                CoreError::invalid_data(format!(
                    "hot Ledger prune watermark {watermark} exists without a canonical tip"
                ))
            })?;
            if watermark > canonical_tip {
                return Err(CoreError::invalid_data(format!(
                    "hot Ledger prune watermark {watermark} exceeds canonical tip {canonical_tip}"
                )));
            }
            if original_tip.is_none_or(|archive_tip| archive_tip < watermark) {
                return Err(CoreError::invalid_data(format!(
                    "static Ledger archive tip {:?} does not cover hot-pruned height {watermark}",
                    original_tip
                )));
            }
        }
        let Some(canonical_tip) = canonical_tip else {
            self.truncate_after(None)?;
            return Ok(StaticArchiveRecovery {
                truncated_blocks: original_tip.map_or(0, |height| height.saturating_add(1)),
                appended_blocks: 0,
                final_tip: None,
                hot_pruned_through,
            });
        };

        let hot = StorageLedgerProvider::new(snapshot);
        if let Some(archive_tip) = original_tip {
            let archived = self.provider();
            let overlap_end = archive_tip.min(canonical_tip);
            let overlap_start = match hot_pruned_through {
                Some(height) => height.checked_add(1),
                None => Some(0),
            };
            if let Some(overlap_start) = overlap_start {
                if overlap_start <= overlap_end {
                    for height in overlap_start..=overlap_end {
                        let archived_hash =
                            archived.block_hash_by_index(height)?.ok_or_else(|| {
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
            hot_pruned_through,
        })
    }
}
