use std::time::Instant;

use tracing::{debug, warn};

use crate::command::ImportBlocksStats;
use crate::service::{BlockchainService, MempoolLike};

impl<S, M> BlockchainService<S, M>
where
    S: crate::service_context::SystemContext,
    M: MempoolLike,
{
    /// Finish a trusted bulk import after all accepted blocks have been staged.
    ///
    /// The import loop owns per-block verification and persistence. This helper
    /// owns the batch-level commit boundary: flush deferred commit handlers,
    /// durable-store commit, header cleanup, stale parked-block removal, and one
    /// final parked-child drain.
    pub(crate) async fn finalize_bulk_import(
        &self,
        imported: usize,
        last_imported_height: Option<u32>,
        stats: &mut ImportBlocksStats,
    ) -> Result<(), String> {
        if imported > 0 {
            let finalization_start = Instant::now();
            let commit_handlers_start = Instant::now();
            if let Err(error) = self.system.flush_bulk_sync_commit_handlers() {
                warn!(
                    target: "neo",
                    imported,
                    error = %error,
                    "bulk import finalization failed before durable store commit"
                );
                stats.finalization_commit_handlers_elapsed += commit_handlers_start.elapsed();
                stats.finalization_elapsed += finalization_start.elapsed();
                return Err(error);
            }
            stats.finalization_commit_handlers_elapsed += commit_handlers_start.elapsed();

            let store_commit_start = Instant::now();
            self.system.commit_to_store();
            stats.finalization_store_commit_elapsed += store_commit_start.elapsed();
            stats.finalization_elapsed += finalization_start.elapsed();
        }

        if let Some(height) = last_imported_height {
            self.header_cache.remove_up_to(height);
            let removed = self.remove_parked_blocks_up_to(height);
            if removed > 0 {
                debug!(
                    target: "neo",
                    removed,
                    height,
                    "removed stale parked blocks after bulk import"
                );
            }
        }

        let drained = self.handle_drain_unverified_blocks().await;
        if drained > 0 {
            debug!(target: "neo", drained, "drained parked unverified blocks after bulk import");
        }

        Ok(())
    }
}
