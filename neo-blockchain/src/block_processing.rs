//! Block verification + persistence loop.
//!
//! The service accepts blocks from peers/consensus, parks out-of-order blocks
//! in a bounded unverified cache, and drains consecutive parked blocks as soon
//! as their parent lands.
//!
//! The native-contract half of C# `Blockchain.Persist` IS wired:
//! when the [`crate::service_context::SystemContext`] exposes a
//! store snapshot, [`BlockchainService::persist_block_sequence`]
//! runs [`crate::native_persist::persist_block_natives`] (genesis
//! initialization + `OnPersist` + `PostPersist` native hooks) over
//! it.

use std::sync::Arc;

use neo_payloads::Block;
use tracing::{debug, error, warn};

use crate::internal::UnverifiedBlock;
use crate::service::BlockchainService;

const DRAIN_BATCH_SIZE: usize = 50;
const MAX_UNVERIFIED_CACHE_SIZE: usize = 20_000;

impl BlockchainService {
    pub(crate) fn park_unverified_block(
        &self,
        block: Arc<Block>,
        relay: bool,
        pre_verified: bool,
    ) -> bool {
        if self.unverified_block_count() >= MAX_UNVERIFIED_CACHE_SIZE {
            warn!(
                target: "neo",
                index = block.index(),
                "unverified block cache is full; dropping future block"
            );
            return false;
        }

        let index = block.index();
        let mut cache = self.unverified_blocks.lock();
        cache
            .entry(index)
            .or_default()
            .push_back(UnverifiedBlock::new(block, relay, pre_verified));
        true
    }

    fn pop_next_unverified_block(&self) -> Option<UnverifiedBlock> {
        let next_index = self.ledger.current_height().saturating_add(1);
        let mut cache = self.unverified_blocks.lock();
        let (next, empty) = {
            let list = cache.get_mut(&next_index)?;
            let next = list.pop_front();
            (next, list.is_empty())
        };
        if empty {
            cache.remove(&next_index);
        }
        next
    }

    /// Persist a consecutive block sequence: run the C#
    /// `Blockchain.Persist` pipeline (native OnPersist + ledger
    /// records, per-transaction Application execution, native
    /// PostPersist) when the system context exposes a store snapshot.
    /// The pipeline stages all writes in a child cache and commits
    /// them into the snapshot only when the whole sequence succeeds
    /// (see [`crate::native_persist`]). Store-less contexts are reserved for
    /// lightweight tests that exercise the command loop without durable
    /// native-contract persistence.
    pub(crate) async fn persist_block_sequence(&self, block: Arc<Block>) -> bool {
        let Some(snapshot) = self.system.store_snapshot() else {
            debug!(
                target: "neo",
                index = block.index(),
                "persist_block_sequence: no store snapshot exposed; skipping durable native persistence for store-less context"
            );
            return true;
        };
        let settings = self.system.settings();
        match crate::native_persist::persist_block_natives(
            Arc::clone(&snapshot),
            Arc::clone(&block),
            settings.as_ref(),
        ) {
            Ok(outcome) => {
                if !self.system.block_committing(
                    block.as_ref(),
                    &snapshot,
                    &outcome.application_executed,
                ) {
                    error!(
                        target: "neo",
                        index = block.index(),
                        "block committing hook failed"
                    );
                    return false;
                }

                debug!(
                    target: "neo",
                    initialized = ?outcome.initialized,
                    engines = outcome.application_executed.len(),
                    "block persistence pipeline completed"
                );
                true
            }
            Err(err) => {
                error!(target: "neo", %err, "block persistence pipeline failed");
                false
            }
        }
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
#[path = "tests/block_processing.rs"]
mod tests;
