use std::time::Instant;

use tracing::warn;

use crate::command::{ImportBlocksReply, ImportBlocksStats};
use crate::import::Import;
use crate::internal::ImportDisposition;
use crate::native_persist::NativePersistOptions;
use crate::service::{BlockchainService, MempoolLike};
use crate::service_context::BlockPersistContext;

mod empty_fast_forward;
mod finalization;
mod persist;
mod verification;

impl<S, M> BlockchainService<S, M>
where
    S: crate::service_context::SystemContext,
    M: MempoolLike,
{
    /// Handle a [`BlockchainCommand::Import`] request.
    pub(crate) async fn handle_import(&self, import: Import) -> ImportBlocksReply {
        let mut imported = 0usize;
        let mut already_durable = 0usize;
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
        let durable_height = self.ledger.current_height();
        let mut deferred_committed_positions = Vec::new();
        let mut import_error = None;
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
                    already_durable += 1;
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
                    break;
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
                        import_error = Some(error.to_string());
                        break;
                    }
                }
                batch_persist_resources_loaded = true;
            }

            if bulk_sync
                && !import.verify
                && let Some(resources) = &batch_persist_resources
                && let Some((fast_forwarded, last_height)) = self.try_bulk_empty_fast_forward(
                    &blocks,
                    position,
                    current_height,
                    resources,
                    &mut stats,
                )
            {
                imported += fast_forwarded;
                last_imported_height = Some(last_height);
                position += fast_forwarded;
                continue;
            }

            if import.verify
                && !self.verify_import_block_for_command(
                    block,
                    current_height,
                    bulk_sync,
                    batch_persist_resources.as_ref(),
                )
            {
                break;
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
                        deferred_committed_positions.push(position);
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
                        import_error = Some(error.to_string());
                        break;
                    }
                }
            }

            if let Err(error) = self
                .persist_import_block_for_command(
                    &blocks[position],
                    bulk_sync,
                    persist_options,
                    persist_context,
                    batch_persist_resources.as_ref(),
                    &mut stats,
                )
                .await
            {
                warn!(target: "neo", %error, height = index, "block import persistence failed");
                import_error = Some(error);
                break;
            }
            imported += 1;
            last_imported_height = Some(index);
            if bulk_sync {
                deferred_committed_positions.push(position);
            }
            position += 1;
            if self.system.should_stop_blockchain_service() {
                import_error.get_or_insert_with(|| {
                    format!(
                        "import stopped after durable block {index}: canonical writer shutdown requested"
                    )
                });
                break;
            }
        }
        if bulk_sync {
            if self.system.should_stop_blockchain_service() {
                self.system.abort_store_commit();
                self.ledger.rewind_to(durable_height);
                return ImportBlocksReply::failed_with_stats(
                    already_durable,
                    stats,
                    import_error.unwrap_or_else(|| {
                        "bulk import aborted after a fatal persistence failure".to_string()
                    }),
                );
            }
            if let Err(error) = self.finalize_bulk_import(imported, &mut stats) {
                self.ledger.rewind_to(durable_height);
                return ImportBlocksReply::failed_with_stats(already_durable, stats, error);
            }
            for position in deferred_committed_positions {
                let committed_hook_start =
                    (!blocks[position].transactions.is_empty()).then(std::time::Instant::now);
                self.system
                    .block_committed_with_context(&blocks[position], persist_context);
                if let Some(start) = committed_hook_start {
                    stats.transaction_committed_hook_elapsed += start.elapsed();
                }
            }
            self.finish_bulk_import_cache_maintenance(last_imported_height)
                .await;
            if self.system.should_stop_blockchain_service() {
                import_error.get_or_insert_with(|| {
                    "bulk import committed durably but canonical writer shutdown was requested"
                        .to_string()
                });
            }
        }
        if let Some(error) = import_error {
            return ImportBlocksReply::failed_with_stats(imported, stats, error);
        }
        ImportBlocksReply::ok_with_stats(imported, stats)
    }
}
