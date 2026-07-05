//! Block verification + persistence loop.
//!
//! The service accepts blocks from peers/consensus, parks out-of-order blocks
//! in a bounded unverified cache, and drains consecutive parked blocks as soon
//! as their parent lands.
//!
//! The native-contract half of C# `Blockchain.Persist` IS wired:
//! when the [`crate::service_context::SystemContext`] exposes a
//! store snapshot, `BlockchainService::persist_block_sequence`
//! stages [`crate::native_persist::stage_block_natives`] (genesis
//! initialization + `OnPersist` + `PostPersist` native hooks) over
//! it and only publishes the staged snapshot after the committing
//! hook succeeds.

use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_payloads::Block;
use neo_storage::DataCache;
use tracing::{debug, error, warn};

use crate::internal::UnverifiedBlock;
use crate::service::{BlockchainService, MempoolLike};
use crate::service_context::BlockPersistContext;

const DRAIN_BATCH_SIZE: usize = 500;
const MAX_UNVERIFIED_CACHE_SIZE: usize = 50_000;

pub(crate) struct BatchPersistResources {
    pub(crate) snapshot: Arc<DataCache>,
    pub(crate) settings: Arc<ProtocolSettings>,
    pub(crate) native_persist: crate::native_persist::NativePersistResources,
}

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
        if self.unverified_block_count() >= MAX_UNVERIFIED_CACHE_SIZE {
            // Cache is full. Clear the oldest entries (highest heights) to make
            // room — this prevents a permanent stall where the cache is full of
            // future blocks that can't be processed because the node is waiting
            // for a specific missing block. The cleared blocks will be
            // re-requested by the network layer's sync timer.
            let mut cache = self.unverified_blocks.lock();
            let count_before = cache.values().map(|v| v.len()).sum::<usize>();
            if count_before >= MAX_UNVERIFIED_CACHE_SIZE {
                // Drop the top 25% of entries (highest block heights).
                let keys_to_drop: Vec<u32> =
                    cache.keys().rev().take(count_before / 4).copied().collect();
                for k in keys_to_drop {
                    cache.remove(&k);
                }
                let dropped = count_before - cache.values().map(|v| v.len()).sum::<usize>();
                warn!(
                    target: "neo",
                    index = block.index(),
                    dropped,
                    "unverified block cache overflow; evicted oldest 25% to prevent permanent stall"
                );
            }
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

    /// Drop parked blocks that are no longer future candidates after a trusted
    /// import advanced the canonical tip.
    pub(crate) fn remove_parked_blocks_up_to(&self, up_to_index: u32) -> usize {
        let mut cache = self.unverified_blocks.lock();
        let keys_to_remove: Vec<u32> = cache
            .range(..=up_to_index)
            .map(|(index, _)| *index)
            .collect();
        let mut removed = 0usize;
        for index in keys_to_remove {
            if let Some(blocks) = cache.remove(&index) {
                removed += blocks.len();
            }
        }
        removed
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
        self.persist_block_sequence_with_options(
            block,
            crate::native_persist::NativePersistOptions::default(),
        )
        .await
    }

    pub(crate) async fn persist_block_sequence_with_options(
        &self,
        block: Arc<Block>,
        options: crate::native_persist::NativePersistOptions,
    ) -> bool {
        let resources = match self.batch_persist_resources(block.index()) {
            Ok(Some(resources)) => resources,
            Ok(None) => return true,
            Err(err) => {
                error!(
                    target: "neo",
                    %err,
                    "block persistence pipeline resource setup failed"
                );
                return false;
            }
        };
        self.persist_block_sequence_with_resources(block, options, &resources)
    }

    pub(crate) fn batch_persist_resources(
        &self,
        index: u32,
    ) -> neo_error::CoreResult<Option<BatchPersistResources>> {
        let Some(snapshot) = self.system.store_snapshot() else {
            debug!(
                target: "neo",
                index,
                "persist_block_sequence: no store snapshot exposed; skipping durable native persistence for store-less context"
            );
            return Ok(None);
        };
        let native_contract_provider = self.system.native_contract_provider().ok_or_else(|| {
            neo_error::CoreError::invalid_operation(
                "persist_block_sequence requires a native-contract provider from SystemContext",
            )
        })?;
        Ok(Some(BatchPersistResources {
            snapshot,
            settings: self.system.settings(),
            native_persist: crate::native_persist::NativePersistResources::from_provider(
                native_contract_provider,
            ),
        }))
    }

    pub(crate) fn persist_block_sequence_with_resources(
        &self,
        block: Arc<Block>,
        options: crate::native_persist::NativePersistOptions,
        resources: &BatchPersistResources,
    ) -> bool {
        let persist_context = if options.capture_replay_artifacts {
            BlockPersistContext::live()
        } else {
            BlockPersistContext::bulk_sync()
        };
        match crate::native_persist::stage_block_natives_with_resources(
            Arc::clone(&resources.snapshot),
            Arc::clone(&block),
            resources.settings.as_ref(),
            options,
            &resources.native_persist,
        ) {
            Ok(staged) => {
                if !self.system.block_committing_with_context(
                    block.as_ref(),
                    staged.snapshot(),
                    &staged.outcome.application_executed,
                    persist_context,
                ) {
                    error!(
                        target: "neo",
                        index = block.index(),
                        "block committing hook failed"
                    );
                    return false;
                }
                staged.commit();

                debug!(
                    target: "neo",
                    initialized = ?staged.outcome.initialized,
                    engines = staged.outcome.application_executed.len(),
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
#[path = "../tests/pipeline/block_processing.rs"]
mod tests;
