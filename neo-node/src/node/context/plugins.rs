//! Durability policy and finalized publication for `DaemonCommitHooks`.
//!
//! These hooks are node-local orchestration. They decide when to run expensive
//! StateService and indexer work before canonical durability, and when to route
//! ApplicationLogs and TokensTracker work through acknowledged finalized
//! delivery. They do not define protocol or storage semantics.

use neo_blockchain::{BlockPersistContext, FinalizedBlock, SyncBatchCommitPolicy};
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_payloads::{ApplicationExecuted, Block};
use neo_storage::persistence::StoreCacheBacking;
use neo_storage::persistence::{Store, TransactionalStore};
use neo_storage::{CacheRead, DataCache, StorageError};
use neo_system::{BlockCommitHooks, CanonicalCommit};
use tracing::{debug, warn};

use super::{CoordinatedNodeStoreWith, DaemonCommitHooks};
use crate::node::static_files::{
    STATIC_ARCHIVE_MAX_DEFERRED_BLOCKS, STATIC_ARCHIVE_PRUNE_BATCH_FRAMES, hot_ledger_prune_target,
};

const COMMITTED_CATCHUP_DISTANCE: u64 = 1_000;
const COMMITTING_CATCHUP_DISTANCE: u64 = 10_000;

impl<P, S, L, T, C> DaemonCommitHooks<P, S, L, T, C>
where
    P: NativeContractProvider + 'static,
    S: Store + 'static,
    L: Store + 'static,
    T: Store + 'static,
    C: Store + 'static,
{
    #[cfg(test)]
    pub(in crate::node) fn block_committing_with_live_tip<B: CacheRead>(
        &self,
        block: &Block,
        snapshot: &DataCache<B>,
        application_executed_list: &[ApplicationExecuted],
        live_tip: u64,
    ) -> bool {
        self.block_committing_with_live_tip_and_context(
            block,
            snapshot,
            application_executed_list,
            live_tip,
            BlockPersistContext::live(),
        )
    }

    fn block_committing_with_live_tip_and_context<B: CacheRead>(
        &self,
        block: &Block,
        snapshot: &DataCache<B>,
        application_executed_list: &[ApplicationExecuted],
        live_tip: u64,
        context: BlockPersistContext,
    ) -> bool {
        if let Some(archive) = &self.static_archive {
            match archive.capture_block(snapshot, block) {
                Ok(record) => self.pending_static_records.lock().push(record),
                Err(error) => {
                    warn!(
                        target: "neo::static_files",
                        height = block.index(),
                        error = %error,
                        "failed to capture finalized Ledger rows for static archive"
                    );
                    return false;
                }
            }
        }

        // During catch-up, skip the expensive per-block hooks:
        // - StateService.on_committing computes the MPT state root per block
        //   (~24ms measured — the dominant sync bottleneck). Validation
        //   profiles can force it on with [state_service].track_during_catchup.
        // - IndexerService.index_block indexes transaction execution results.
        // Deferred hooks resume near the live tip. Trusted replay always skips
        // them; verified sync can freeze the same catch-up decision per batch.
        let block_index = block.index();
        let catching_up = context.skips_live_observers()
            || (context.uses_dynamic_peer_tip()
                && live_tip > 0
                && u64::from(block_index).saturating_add(COMMITTING_CATCHUP_DISTANCE) < live_tip);
        let should_track_state = self.state_service.is_some()
            && (!catching_up || self.state_service_track_during_catchup);
        let should_index_live = !catching_up
            && self
                .indexer_service
                .as_ref()
                .is_some_and(|indexer| indexer.can_append_contiguous_block(block));
        // A non-coordinated StateService or persistent indexer can make an
        // independent store durable before Ledger. Coordinated MDBX
        // StateService and post-canonical projections do not arm this marker.
        let has_persistent_indexer = should_index_live
            && self
                .indexer_service
                .as_ref()
                .is_some_and(|indexer| indexer.is_persistent());
        let state_requires_recovery_marker = should_track_state
            && self
                .state_service
                .as_ref()
                .is_some_and(|state_service| !state_service.is_coordinated());
        if (state_requires_recovery_marker || has_persistent_indexer)
            && !self.replay_guard.begin_observer_commit()
        {
            return false;
        }

        if let Some(state_service) = &self.state_service {
            let state_ok = if should_track_state {
                if catching_up {
                    state_service.on_committing_deferred(block.index(), snapshot)
                } else {
                    state_service.on_committing(block.index(), snapshot)
                }
            } else {
                true
            };
            if !state_ok {
                return false;
            }
        }

        if catching_up {
            return true;
        }

        if let Some(indexer) = &self.indexer_service
            && should_index_live
        {
            if let Err(error) =
                indexer.index_block_with_application_executions(block, application_executed_list)
            {
                warn!(
                    target: "neo::indexer",
                    height = block.index(),
                    persistent = indexer.is_persistent(),
                    error = %error,
                    "failed to index block application executions"
                );
                if indexer.is_persistent() {
                    return false;
                }
            }
        } else if self.indexer_service.is_some() && !catching_up {
            debug!(
                target: "neo::indexer",
                height = block_index,
                "deferred live index write until the durable Index stage fills the preceding gap"
            );
        }
        true
    }
}

impl<P, S, L, T, C> BlockCommitHooks<C> for DaemonCommitHooks<P, S, L, T, C>
where
    P: NativeContractProvider + 'static,
    S: Store + 'static,
    L: Store + 'static,
    T: Store + 'static,
    C: TransactionalStore + CoordinatedNodeStoreWith<S> + 'static,
{
    fn requires_replay_artifacts(&self, _block: &Block, context: BlockPersistContext) -> bool {
        if context.skips_live_observers() {
            return false;
        }

        self.finalized_projections.has_consumers() || self.indexer_service.is_some()
    }

    fn block_committing(
        &self,
        block: &Block,
        snapshot: &DataCache<StoreCacheBacking<C>>,
        application_executed: &[ApplicationExecuted],
        live_tip: u64,
        context: BlockPersistContext,
    ) -> bool {
        self.block_committing_with_live_tip_and_context(
            block,
            snapshot,
            application_executed,
            live_tip,
            context,
        )
    }

    async fn block_finalized(
        &self,
        finalized: FinalizedBlock<StoreCacheBacking<C>>,
        live_tip: u64,
    ) -> Result<(), String> {
        let block_index = finalized.block().index();
        let context = finalized.context();
        let catching_up = context.skips_live_observers()
            || (context.uses_dynamic_peer_tip()
                && live_tip > 0
                && u64::from(block_index).saturating_add(COMMITTED_CATCHUP_DISTANCE) < live_tip);
        if catching_up || !self.finalized_projections.has_consumers() {
            return Ok(());
        }
        self.finalized_blocks
            .publish(finalized)
            .await
            .map_err(|error| error.to_string())
    }

    fn sync_batch_commit_policy(
        &self,
        start_height: u32,
        end_height: u32,
        live_tip: u64,
    ) -> SyncBatchCommitPolicy {
        let batch_blocks = u64::from(end_height)
            .saturating_sub(u64::from(start_height))
            .saturating_add(1);
        if self.static_archive.is_some()
            && batch_blocks
                > u64::try_from(STATIC_ARCHIVE_MAX_DEFERRED_BLOCKS)
                    .expect("archive batch bound fits u64")
        {
            return SyncBatchCommitPolicy::PerBlock;
        }
        let observers_skipped_for_entire_batch = live_tip > 0
            && u64::from(end_height).saturating_add(COMMITTING_CATCHUP_DISTANCE) < live_tip;
        let has_post_canonical_staging = self.finalized_projections.has_consumers();

        if has_post_canonical_staging && !observers_skipped_for_entire_batch {
            return SyncBatchCommitPolicy::PerBlock;
        }

        if observers_skipped_for_entire_batch {
            SyncBatchCommitPolicy::DeferredCatchUp
        } else {
            SyncBatchCommitPolicy::DeferredLive
        }
    }

    fn flush_deferred(&self) -> Result<(), String> {
        if let Some(state_service) = &self.state_service {
            state_service
                .flush_result()
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn fence_precommit_durability(&self) -> Result<(), String> {
        if let Some(state_service) = &self.state_service {
            if state_service.is_coordinated() {
                state_service.flush_result().map_err(str::to_string)?;
            } else {
                state_service.flush_durable_result()?;
            }
        }
        if let Some(indexer) = &self.indexer_service {
            indexer
                .flush_durable()
                .map_err(|error| format!("indexer durability fence failed: {error}"))?;
        }
        if let Some(archive) = &self.static_archive {
            let pending = std::mem::take(&mut *self.pending_static_records.lock());
            if !pending.is_empty() {
                archive
                    .stage_records(pending)
                    .map_err(|error| format!("static archive durability fence failed: {error}"))?;
            }
        }
        Ok(())
    }

    fn commit_canonical<K>(&self, canonical_commit: &mut K) -> Result<(), String>
    where
        K: CanonicalCommit<C>,
    {
        if let Err(error) = self.fence_precommit_durability() {
            canonical_commit.discard_pending();
            if let Some(state_service) = &self.state_service {
                state_service.discard_pending_coordinated();
            }
            return Err(format!("pre-commit durability fence failed: {error}"));
        }

        if let Some(state_service) = &self.state_service
            && state_service.is_coordinated()
        {
            let coordinated =
                state_service.commit_pending_coordinated(|state_backing, state_overlay| {
                    canonical_commit
                        .commit_durable_with(|canonical, canonical_overlay| {
                            canonical.commit_node_overlays(
                                canonical_overlay,
                                state_backing,
                                state_overlay,
                            )
                        })
                        .map_err(|error| StorageError::CommitFailed(error.to_string()))
                });
            match coordinated {
                Ok(Some(_roots)) => return Ok(()),
                Ok(None) => {}
                Err(error) => {
                    canonical_commit.discard_pending();
                    return Err(error);
                }
            }
        }

        canonical_commit.commit_durable()
    }

    fn canonical_commit_succeeded(&self) {
        self.replay_guard.canonical_commit_succeeded();
        let Some(archive) = &self.static_archive else {
            return;
        };
        if let Err(error) = archive.publish_staged_records() {
            warn!(
                target: "neo::static_files",
                error = %error,
                "canonical Ledger committed but staged archive publication failed; startup recovery will publish or replay the suffix"
            );
            self.replay_guard
                .request_recoverable_restart("staged static archive publication failed");
            return;
        }

        let Some(pruning) = self.hot_ledger_pruning.read().clone() else {
            return;
        };
        let Some(target) = archive
            .tip()
            .and_then(|tip| hot_ledger_prune_target(tip, pruning.retention_blocks))
        else {
            return;
        };
        match archive.prune_hot_through(
            pruning.store.as_ref(),
            target,
            STATIC_ARCHIVE_PRUNE_BATCH_FRAMES,
        ) {
            Ok(outcome) => {
                debug!(
                    target: "neo::static_files",
                    pruned_through = outcome.pruned_through,
                    deleted_rows = outcome.deleted_rows,
                    processed_frames = outcome.processed_frames,
                    "pruned archived Ledger rows from the hot store"
                );
            }
            Err(error) => {
                warn!(
                    target: "neo::static_files",
                    error = %error,
                    "static archive published but hot Ledger pruning failed"
                );
                self.replay_guard
                    .request_recoverable_restart("hot Ledger pruning failed");
            }
        }
    }

    fn canonical_commit_failed(&self, reason: &str) {
        self.pending_static_records.lock().clear();
        if let Some(state_service) = &self.state_service {
            state_service.discard_pending_coordinated();
        }
        self.replay_guard.canonical_commit_failed(reason);
    }

    fn finalized_delivery_failed(&self, reason: &str) {
        self.replay_guard.request_recoverable_restart(reason);
    }

    fn should_stop_blockchain_service(&self) -> bool {
        self.replay_guard.shutdown_requested()
    }

    fn allows_empty_block_fast_forward(&self) -> bool {
        self.static_archive.is_none()
            && self.state_service.is_none()
            && self.indexer_service.is_none()
            && !self.finalized_projections.has_consumers()
    }

    fn allows_empty_block_committing_fast_forward(&self) -> bool {
        (self.static_archive.is_some() || self.state_service.is_some())
            && self.indexer_service.is_none()
            && !self.finalized_projections.has_consumers()
    }
}
