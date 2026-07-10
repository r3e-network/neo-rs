//! Block verification + persistence loop.
//!
//! The service accepts blocks from peers/consensus, parks out-of-order blocks
//! in a bounded unverified cache, and drains consecutive parked blocks as soon
//! as their parent lands.
//!
//! The native-contract half of C# `Blockchain.Persist` IS wired:
//! when the [`crate::service_context::SystemContext`] exposes a
//! store snapshot, `BlockchainService::persist_block_sequence`
//! stages [`crate::native_persist::stage_block_natives_with_resources`]
//! (genesis initialization + `OnPersist` + `PostPersist` native hooks) over it
//! and only publishes the staged snapshot after the committing hook succeeds.

use std::sync::Arc;

use neo_payloads::Block;
use tracing::warn;

use crate::internal::UnverifiedBlock;
use crate::service::{BlockchainService, MempoolLike};

mod persist;

pub(crate) use persist::BatchPersistResources;

const DRAIN_BATCH_SIZE: usize = 500;
const MAX_UNVERIFIED_CACHE_SIZE: usize = 50_000;

impl<S, M> BlockchainService<S, M>
where
    S: crate::service_context::SystemContext,
    M: MempoolLike,
{
    pub(crate) fn park_unverified_block(
        &self,
        block: Arc<Block>,
        relay: bool,
        pre_verified: bool,
    ) -> bool {
        let index = block.index();
        let dropped = {
            let mut cache = self.unverified_blocks.lock();
            let dropped = if cache.len() >= MAX_UNVERIFIED_CACHE_SIZE {
                // Keep the closest future heights and discard exactly one
                // quarter of the farthest-future blocks. The network sync
                // scheduler can request discarded inventory again after the
                // missing gap lands.
                let eviction_target = (cache.len() / 4).max(1);
                cache.evict_highest(eviction_target)
            } else {
                0
            };
            cache.push(UnverifiedBlock::new(block, relay, pre_verified));
            dropped
        };
        if dropped > 0 {
            warn!(
                target: "neo",
                index,
                dropped,
                "unverified block cache overflow; evicted farthest-future blocks"
            );
        }
        true
    }

    fn pop_next_unverified_block(&self) -> Option<UnverifiedBlock> {
        let next_index = self.ledger.current_height().saturating_add(1);
        self.unverified_blocks.lock().pop_front(next_index)
    }

    /// Drop parked blocks that are no longer future candidates after a trusted
    /// import advanced the canonical tip.
    pub(crate) fn remove_parked_blocks_up_to(&self, up_to_index: u32) -> usize {
        self.unverified_blocks.lock().remove_up_to(up_to_index)
    }

    /// Drain the unverified block cache, persisting up to one bounded batch of
    /// consecutive blocks whose parents have landed.
    pub(crate) async fn handle_drain_unverified_blocks(&self) -> usize {
        let mut drained = 0usize;
        while drained < DRAIN_BATCH_SIZE {
            let Some(candidate) = self.pop_next_unverified_block() else {
                break;
            };
            let index = candidate.block.index();
            match self
                .persist_next_expected_block(
                    candidate.block,
                    candidate.relay,
                    candidate.pre_verified,
                )
                .await
            {
                Ok(()) => drained += 1,
                Err(err) => {
                    warn!(
                        target: "neo",
                        index,
                        %err,
                        "dropping parked block that failed verification/persistence"
                    );
                }
            }
        }
        drained
    }
}

#[cfg(test)]
#[path = "../tests/pipeline/block_processing.rs"]
mod tests;
