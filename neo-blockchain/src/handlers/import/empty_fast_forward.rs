use std::sync::Arc;
use std::time::Instant;

use neo_payloads::Block;
use tracing::debug;

use crate::block_processing::BatchPersistResources;
use crate::command::ImportBlocksStats;
use crate::empty_block_fast_forward::stage_empty_block_fast_forward;
use crate::service::{BlockchainService, MempoolLike};

impl<S, M> BlockchainService<S, M>
where
    S: crate::service_context::SystemContext,
    M: MempoolLike,
{
    /// Try the state-equivalent empty-block fast path for a trusted bulk import run.
    ///
    /// The run is borrowed from the import batch so empty blocks are not cloned
    /// before the fast-forward path has a chance to consume them. On success the
    /// helper advances durable state and the hot ledger tip, then returns the
    /// accepted run length and last height for the outer accepted-prefix loop.
    pub(crate) fn try_bulk_empty_fast_forward(
        &self,
        blocks: &[Block],
        position: usize,
        current_height: u32,
        resources: &BatchPersistResources<S::NativeProvider, S::CacheBacking>,
        stats: &mut ImportBlocksStats,
    ) -> Option<(usize, u32)> {
        if !self.system.allows_empty_block_fast_forward() {
            return None;
        }
        let run = Self::collect_empty_fast_forward_run(
            blocks,
            position,
            current_height,
            resources.settings.as_ref(),
            &resources.native_persist,
        );
        if run.is_empty() {
            return None;
        }

        let empty_start = Instant::now();
        match stage_empty_block_fast_forward(
            Arc::clone(&resources.snapshot),
            &run,
            resources.settings.as_ref(),
            crate::native_persist::NativePersistOptions {
                capture_replay_artifacts: false,
            },
            crate::service_context::BlockPersistContext::bulk_sync(),
            &resources.native_persist,
            current_height,
        ) {
            Ok(staged) => {
                staged.commit();
                stats.empty_blocks += run.len();
                stats.empty_elapsed += empty_start.elapsed();
                let last_height = run.last()?.index();
                // Fast-forward is only enabled when no component needs the
                // per-block observer stream. Keep ledger history in durable
                // storage and only advance the hot in-memory tip for the batch.
                self.ledger.record_tip(last_height);
                Some((run.len(), last_height))
            }
            Err(error) => {
                debug!(
                    target: "neo::sync",
                    height = blocks[position].index(),
                    error = %error,
                    "empty-block fast-forward fell back to normal persistence"
                );
                None
            }
        }
    }
}
