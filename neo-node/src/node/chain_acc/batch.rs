//! Batch accounting and dispatch helpers for trusted `chain.acc` imports.
//!
//! This module owns the import-batch state machine used by the node-facing
//! chain accumulator reader. It tracks composition while blocks are streamed
//! from disk so the import path can report empty-block and transaction-block
//! throughput without rescanning a batch before dispatch.

use std::time::{Duration, Instant};

use neo_blockchain::{BlockchainHandle, ImportBlocksStats};
use neo_payloads::block::Block;
use neo_runtime::BlockImport;

use super::{IMPORT_BATCH_SIZE, LocalLedgerTip};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct PendingChainAccBatch {
    pub(super) len: usize,
    pub(super) composition: ChainAccImportComposition,
    pub(super) tip: Option<LocalLedgerTip>,
}

impl PendingChainAccBatch {
    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    pub(super) fn record_pushed(&mut self, block: &Block) {
        self.len += 1;
        self.tip = Some(LocalLedgerTip {
            height: block.index(),
            hash: block.hash(),
        });
        let tx_count = block.transactions.len() as u64;
        if tx_count == 0 {
            self.composition.empty_blocks += 1;
        } else {
            self.composition.transaction_blocks += 1;
            self.composition.transactions += tx_count;
        }
    }

    pub(super) fn should_flush(&self, batch_len: usize) -> bool {
        debug_assert_eq!(self.len, batch_len);
        batch_len >= IMPORT_BATCH_SIZE
    }
}

pub(super) struct ChainAccBatchImportResult {
    pub(super) len: usize,
    pub(super) imported: usize,
    pub(super) elapsed: Duration,
    pub(super) composition: ChainAccImportComposition,
    pub(super) stats: ImportBlocksStats,
    pub(super) tip: Option<LocalLedgerTip>,
}

impl ChainAccBatchImportResult {
    pub(super) fn fully_imported(&self) -> bool {
        self.imported == self.len
    }
}

pub(super) async fn import_chain_acc_batch(
    handle: &BlockchainHandle,
    batch_blocks: Vec<Block>,
    composition: ChainAccImportComposition,
    tip: Option<LocalLedgerTip>,
    verify: bool,
) -> anyhow::Result<ChainAccBatchImportResult> {
    let len = batch_blocks.len();
    if verify {
        for block in &batch_blocks {
            handle
                .check(block)
                .await
                .map_err(|err| anyhow::anyhow!("chain.acc block preflight failed: {err}"))?;
        }
    }
    let start = Instant::now();
    let reply = handle
        .import_blocks_bulk_detailed(batch_blocks, verify)
        .await?;
    let elapsed = start.elapsed();
    if let Some(error) = reply.error {
        anyhow::bail!(
            "block import finalization failed after importing {} blocks: {error}",
            reply.imported
        );
    }
    Ok(ChainAccBatchImportResult {
        len,
        imported: reply.imported,
        elapsed,
        composition,
        stats: reply.stats,
        tip,
    })
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ChainAccImportComposition {
    pub(super) empty_blocks: u64,
    pub(super) empty_only_blocks: u64,
    pub(super) empty_fast_path_blocks: u64,
    pub(super) transaction_blocks: u64,
    pub(super) transactions: u64,
    pub(super) empty_elapsed: Duration,
    pub(super) transaction_elapsed: Duration,
    pub(super) transaction_block_clone_elapsed: Duration,
    pub(super) transaction_ledger_insert_elapsed: Duration,
    pub(super) transaction_finalized_delivery_elapsed: Duration,
    pub(super) finalization_elapsed: Duration,
    pub(super) finalization_commit_handlers_elapsed: Duration,
    pub(super) finalization_store_commit_elapsed: Duration,
}

impl ChainAccImportComposition {
    pub(super) fn record_imported(
        &mut self,
        batch: Self,
        imported: usize,
        elapsed: Duration,
        stats: ImportBlocksStats,
    ) {
        if imported == 0 {
            return;
        }
        let imported = imported as u64;
        let batch_blocks = batch.empty_blocks + batch.transaction_blocks;
        if imported >= batch_blocks {
            self.empty_blocks += batch.empty_blocks;
            self.transaction_blocks += batch.transaction_blocks;
            self.transactions += batch.transactions;
            if stats.has_composition() {
                if stats.empty_blocks > 0 {
                    let empty_blocks = stats.empty_blocks as u64;
                    self.empty_fast_path_blocks += empty_blocks;
                    if stats.transaction_blocks == 0 {
                        self.empty_only_blocks += empty_blocks;
                    }
                    self.empty_elapsed += stats.empty_elapsed;
                }
                if stats.transaction_blocks > 0 {
                    self.transaction_elapsed += stats.transaction_elapsed;
                    self.transaction_block_clone_elapsed += stats.transaction_block_clone_elapsed;
                    self.transaction_ledger_insert_elapsed +=
                        stats.transaction_ledger_insert_elapsed;
                    self.transaction_finalized_delivery_elapsed +=
                        stats.transaction_finalized_delivery_elapsed;
                }
                self.finalization_elapsed += stats.finalization_elapsed;
                self.finalization_commit_handlers_elapsed +=
                    stats.finalization_commit_handlers_elapsed;
                self.finalization_store_commit_elapsed += stats.finalization_store_commit_elapsed;
            } else if batch.transaction_blocks > 0 {
                self.transaction_elapsed += elapsed;
            } else if batch.empty_blocks > 0 {
                self.empty_only_blocks += batch.empty_blocks;
                self.empty_fast_path_blocks += batch.empty_blocks;
                self.empty_elapsed += elapsed;
            }
        }
    }

    pub(super) fn empty_block_import_seconds(&self) -> f64 {
        self.empty_elapsed.as_secs_f64()
    }

    pub(super) fn empty_blocks_per_second(&self) -> f64 {
        let elapsed = self.empty_block_import_seconds();
        if elapsed > 0.0 {
            self.empty_fast_path_blocks as f64 / elapsed
        } else {
            0.0
        }
    }

    pub(super) fn transaction_block_import_seconds(&self) -> f64 {
        self.transaction_elapsed.as_secs_f64()
    }

    pub(super) fn transaction_block_clone_seconds(&self) -> f64 {
        self.transaction_block_clone_elapsed.as_secs_f64()
    }

    pub(super) fn transaction_ledger_insert_seconds(&self) -> f64 {
        self.transaction_ledger_insert_elapsed.as_secs_f64()
    }

    pub(super) fn transaction_finalized_delivery_seconds(&self) -> f64 {
        self.transaction_finalized_delivery_elapsed.as_secs_f64()
    }

    pub(super) fn transaction_blocks_per_second(&self) -> f64 {
        let elapsed = self.transaction_block_import_seconds();
        if elapsed > 0.0 {
            self.transaction_blocks as f64 / elapsed
        } else {
            0.0
        }
    }

    pub(super) fn finalization_seconds(&self) -> f64 {
        self.finalization_elapsed.as_secs_f64()
    }

    pub(super) fn finalization_commit_handlers_seconds(&self) -> f64 {
        self.finalization_commit_handlers_elapsed.as_secs_f64()
    }

    pub(super) fn finalization_store_commit_seconds(&self) -> f64 {
        self.finalization_store_commit_elapsed.as_secs_f64()
    }

    fn accounted_elapsed(&self) -> Duration {
        self.empty_elapsed
            + self.transaction_elapsed
            + self.transaction_block_clone_elapsed
            + self.transaction_ledger_insert_elapsed
            + self.transaction_finalized_delivery_elapsed
            + self.finalization_elapsed
    }

    pub(super) fn unclassified_import_seconds(&self, total: Duration) -> f64 {
        total
            .checked_sub(self.accounted_elapsed())
            .unwrap_or_default()
            .as_secs_f64()
    }
}

pub(super) fn take_import_batch(batch: &mut Vec<Block>, more_blocks_remain: bool) -> Vec<Block> {
    if more_blocks_remain {
        let next_batch = Vec::with_capacity(batch.capacity().max(IMPORT_BATCH_SIZE));
        std::mem::replace(batch, next_batch)
    } else {
        std::mem::take(batch)
    }
}
