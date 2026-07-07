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

            if !self
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
                return ImportBlocksReply::ok_with_stats(imported, stats);
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
