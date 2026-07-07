use std::sync::Arc;
use std::time::Instant;

use tracing::{debug, warn};

use crate::command::{ImportBlocksReply, ImportBlocksStats};
use crate::empty_block_fast_forward::stage_empty_block_fast_forward;
use crate::import::Import;
use crate::internal::ImportDisposition;
use crate::native_persist::NativePersistOptions;
use crate::service::{BlockchainService, MempoolLike};
use crate::service_context::BlockPersistContext;

mod finalization;
mod verification;

impl<S, M> BlockchainService<S, M>
where
    S: crate::service_context::SystemContext,
    M: MempoolLike,
{
    /// Handle a [`BlockchainCommand::Import`] request.
    pub(crate) async fn handle_import(&self, import: Import) -> ImportBlocksReply {
        let mut imported = 0usize;
        let mut stats = ImportBlocksStats::default();
        let bulk_sync = import.bulk_sync;
        let persist_options = if bulk_sync {
            NativePersistOptions {
                capture_replay_artifacts: false,
            }
        } else {
            NativePersistOptions::default()
        };
        let persist_context = if bulk_sync {
            BlockPersistContext::bulk_sync()
        } else {
            BlockPersistContext::live()
        };
        let blocks = import.blocks;
        let mut batch_persist_resources = None;
        let mut batch_persist_resources_loaded = false;
        let mut last_imported_height = None;
        let mut position = 0usize;
        while position < blocks.len() {
            let block = &blocks[position];
            let index = block.index();
            let current_height = self.ledger.current_height();
            match ImportDisposition::classify_import_block(current_height, index) {
                ImportDisposition::AlreadySeen => {
                    imported += 1;
                    position += 1;
                    continue;
                }
                ImportDisposition::FutureGap => {
                    warn!(
                        target: "neo",
                        expected = current_height + 1,
                        actual = index,
                        "import block out of sequence"
                    );
                    return ImportBlocksReply::ok_with_stats(imported, stats);
                }
                ImportDisposition::NextExpected => {}
            }

            if bulk_sync && !batch_persist_resources_loaded {
                match self.batch_persist_resources(index) {
                    Ok(resources) => {
                        batch_persist_resources = resources;
                    }
                    Err(error) => {
                        warn!(
                            target: "neo",
                            %error,
                            height = index,
                            "import aborted: native persistence resource setup failed"
                        );
                        return ImportBlocksReply::ok_with_stats(imported, stats);
                    }
                }
                batch_persist_resources_loaded = true;
            }

            if bulk_sync
                && !import.verify
                && self.system.allows_empty_block_fast_forward()
                && let Some(resources) = &batch_persist_resources
            {
                let run = Self::collect_empty_fast_forward_run(
                    &blocks,
                    position,
                    current_height,
                    resources.settings.as_ref(),
                    &resources.native_persist,
                );
                if !run.is_empty() {
                    let empty_start = Instant::now();
                    match stage_empty_block_fast_forward(
                        Arc::clone(&resources.snapshot),
                        &run,
                        resources.settings.as_ref(),
                        persist_options,
                        persist_context,
                        &resources.native_persist,
                        current_height,
                    ) {
                        Ok(staged) => {
                            staged.commit();
                            stats.empty_blocks += run.len();
                            stats.empty_elapsed += empty_start.elapsed();
                            if let Some(last_block) = run.last() {
                                // Fast-forward is only enabled when no component
                                // needs the per-block observer stream. Keep
                                // ledger history in the durable store and only
                                // advance the hot in-memory tip for the batch.
                                self.ledger.record_tip(last_block.index());
                                imported += run.len();
                                last_imported_height = Some(last_block.index());
                            }
                            position += run.len();
                            continue;
                        }
                        Err(error) => {
                            debug!(
                                target: "neo::sync",
                                height = index,
                                error = %error,
                                "empty-block fast-forward fell back to normal persistence"
                            );
                        }
                    }
                }
            }

            if import.verify
                && !self
                    .verify_import_block_for_command(
                        block,
                        current_height,
                        bulk_sync,
                        batch_persist_resources.as_ref(),
                    )
                    .await
            {
                return ImportBlocksReply::ok_with_stats(imported, stats);
            }

            if bulk_sync && let Some(resources) = &batch_persist_resources {
                let empty_start = Instant::now();
                match self.persist_empty_block_with_committing_fast_forward(
                    block,
                    current_height,
                    resources,
                    persist_options,
                    persist_context,
                ) {
                    Ok(true) => {
                        stats.empty_blocks += 1;
                        stats.empty_elapsed += empty_start.elapsed();
                        imported += 1;
                        last_imported_height = Some(index);
                        position += 1;
                        continue;
                    }
                    Ok(false) => {}
                    Err(error) => {
                        warn!(
                            target: "neo",
                            height = index,
                            %error,
                            "import aborted: empty-block committing fast-forward failed"
                        );
                        return ImportBlocksReply::ok_with_stats(imported, stats);
                    }
                }
            }

            // C# Blockchain.OnImport runs `Persist(block)` - the state
            // transition - before the block becomes the new tip.
            let transaction_block = !blocks[position].transactions.is_empty();
            let clone_start = transaction_block.then(Instant::now);
            let block = Arc::new(blocks[position].clone());
            if let Some(start) = clone_start {
                stats.transaction_block_clone_elapsed += start.elapsed();
            }
            let transaction_block = !block.transactions.is_empty();
            let transaction_start = transaction_block.then(Instant::now);
            let persisted = if bulk_sync {
                if let Some(resources) = &batch_persist_resources {
                    self.persist_block_sequence_with_resources(
                        Arc::clone(&block),
                        persist_options,
                        resources,
                    )
                } else {
                    self.persist_block_sequence_with_options(Arc::clone(&block), persist_options)
                        .await
                }
            } else {
                self.persist_block_sequence_with_options(Arc::clone(&block), persist_options)
                    .await
            };
            if !persisted {
                warn!(
                    target: "neo",
                    height = index,
                    "import aborted: native persistence pipeline failed"
                );
                return ImportBlocksReply::ok_with_stats(imported, stats);
            }
            if let Some(start) = transaction_start {
                stats.transaction_blocks += 1;
                stats.transaction_elapsed += start.elapsed();
            }

            let ledger_insert_start = transaction_block.then(Instant::now);
            if let Err(error) = self.ledger.insert_block_arc(Arc::clone(&block)) {
                warn!(
                    target: "neo",
                    %error,
                    height = index,
                    "failed to import block into ledger cache"
                );
                return ImportBlocksReply::ok_with_stats(imported, stats);
            }
            if let Some(start) = ledger_insert_start {
                stats.transaction_ledger_insert_elapsed += start.elapsed();
            }

            // Normal live imports flush each block immediately. Trusted
            // bulk-sync imports keep staging into the shared snapshot and flush
            // once after the accepted batch, avoiding one RocksDB commit per
            // block while preserving per-block native/state transitions.
            if !bulk_sync {
                self.system.commit_to_store();
            }
            let committed_hook_start = transaction_block.then(Instant::now);
            self.system
                .block_committed_with_context(block.as_ref(), persist_context);
            if let Some(start) = committed_hook_start {
                stats.transaction_committed_hook_elapsed += start.elapsed();
            }

            // Cold-start bulk sync imports a trusted local chain.acc package,
            // so it stays on canonical state transitions only. Live import and
            // peer-relay paths still mirror C# MemPool.UpdatePoolForBlockPersisted
            // per block.
            if !bulk_sync {
                self.mempool.block_persisted(block.as_ref());
                self.reverify_mempool_after_persist(
                    index,
                    self.system.settings().max_transactions_per_block as usize,
                );
            }
            if !bulk_sync {
                self.header_cache.remove_up_to(index);
            }

            if !bulk_sync {
                let drained = self.handle_drain_unverified_blocks().await;
                if drained > 0 {
                    debug!(target: "neo", drained, "drained parked unverified blocks after import");
                }
            }
            imported += 1;
            last_imported_height = Some(index);
            position += 1;
        }
        if bulk_sync {
            if let Err(error) = self
                .finalize_bulk_import(imported, last_imported_height, &mut stats)
                .await
            {
                return ImportBlocksReply::failed_with_stats(imported, stats, error);
            }
        }
        ImportBlocksReply::ok_with_stats(imported, stats)
    }
}
