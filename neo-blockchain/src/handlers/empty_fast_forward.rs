use std::sync::Arc;

use neo_error::{CoreError, CoreResult};
use neo_execution::NativeContract;
use neo_payloads::Block;
use tracing::debug;

use crate::block_processing::BatchPersistResources;
use crate::empty_block_fast_forward::{
    MAX_EMPTY_BLOCK_FAST_FORWARD_BLOCKS, stage_empty_block_fast_forward,
};
use crate::native_persist::NativePersistOptions;
use crate::service::{BlockchainService, MempoolLike};
use crate::service_context::BlockPersistContext;

impl<S, M> BlockchainService<S, M>
where
    S: crate::service_context::SystemContext,
    M: MempoolLike,
{
    pub(super) fn collect_empty_fast_forward_run<'a>(
        blocks: &'a [Block],
        start_position: usize,
        current_height: u32,
        settings: &neo_config::ProtocolSettings,
        resources: &crate::native_persist::NativePersistResources<S::NativeProvider>,
    ) -> Vec<&'a Block> {
        let committee_count = settings.committee_members_count();
        if committee_count == 0 {
            return Vec::new();
        }

        let mut run = Vec::new();
        for block in blocks.iter().skip(start_position) {
            if run.len() >= MAX_EMPTY_BLOCK_FAST_FORWARD_BLOCKS {
                break;
            }
            let expected = current_height.saturating_add(1 + run.len() as u32);
            let height = block.index();
            if height != expected {
                break;
            }
            if !block.transactions.is_empty()
                || block.header.merkle_root() != &neo_primitives::UInt256::zero()
            {
                break;
            }
            let native_cut = resources.contracts().iter().any(|contract| {
                let (initialize, _hardforks) = contract.is_initialize_block(settings, height);
                initialize
                    || (contract.is_active(settings, height)
                        && !contract.supports_empty_block_fast_forward())
            });
            if native_cut {
                break;
            }
            run.push(block);
        }
        run
    }

    pub(super) fn persist_empty_block_with_committing_fast_forward(
        &self,
        block: &Block,
        current_height: u32,
        resources: &BatchPersistResources<S::NativeProvider, S::CacheBacking>,
        persist_options: NativePersistOptions,
        persist_context: BlockPersistContext,
    ) -> CoreResult<bool> {
        if !persist_context.bulk_sync
            || persist_options.capture_replay_artifacts
            || !self.system.allows_empty_block_committing_fast_forward()
            || !block.transactions.is_empty()
            || block.header.merkle_root() != &neo_primitives::UInt256::zero()
            || block.index() != current_height.saturating_add(1)
        {
            return Ok(false);
        }
        let block_hash = Self::try_block_hash(block)?;

        let single = std::slice::from_ref(block);
        let staged = match stage_empty_block_fast_forward(
            Arc::clone(&resources.snapshot),
            single,
            resources.settings.as_ref(),
            persist_options,
            persist_context,
            &resources.native_persist,
            current_height,
        ) {
            Ok(staged) => staged,
            Err(error) => {
                debug!(
                    target: "neo::sync",
                    height = block.index(),
                    error = %error,
                    "empty-block committing fast-forward fell back to normal persistence"
                );
                return Ok(false);
            }
        };

        if !self.system.block_committing_with_context(
            block,
            staged.snapshot(),
            &[],
            persist_context,
        ) {
            return Err(CoreError::other(format!(
                "block {} committing hook failed",
                block.index()
            )));
        }

        staged.commit();
        let block = Arc::new(block.clone());
        self.ledger
            .insert_block_arc_with_hash(Arc::clone(&block), block_hash);
        Ok(true)
    }
}
