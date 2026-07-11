//! Read-side plugin and deferred commit-hook dispatch for `DaemonContext`.
//!
//! These hooks are node-local orchestration. They decide when to run expensive
//! StateService, indexer, ApplicationLogs, and TokensTracker work around bulk
//! sync, but do not define the underlying protocol or storage semantics.

use std::sync::Arc;

use neo_blockchain::{BlockPersistContext, SyncBatchCommitPolicy};
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_payloads::{ApplicationExecuted, Block, CommittedHandler, CommittingHandler};
use neo_storage::persistence::store::Store;
use neo_storage::{CacheRead, DataCache};
use neo_system::BlockCommitHooks;
use tracing::warn;

use super::DaemonCommitHooks;

const COMMITTED_CATCHUP_DISTANCE: u64 = 1_000;
const COMMITTING_CATCHUP_DISTANCE: u64 = 10_000;

impl<P, S, L, T> DaemonCommitHooks<P, S, L, T>
where
    P: NativeContractProvider + 'static,
    S: Store + 'static,
    L: Store + 'static,
    T: Store + 'static,
{
    fn block_committed_with_live_tip_and_context(
        &self,
        block: &Block,
        live_tip: u64,
        context: BlockPersistContext,
    ) {
        // During initial catch-up, skip the application-logs and tokens-tracker
        // indexing handlers (C# does the same during chain.acc import). These
        // index every transaction's execution result per block, which is O(N)
        // expensive and dominates sync time (measured: 30 blocks/min WITH
        // indexing vs 200+ WITHOUT). Once the node is near the live tip
        // (within ~1000 blocks), we enable full indexing for live operation.
        //
        // The plugin services can backfill later via their own catch-up path
        // if needed; the priority during cold sync is reaching consensus tip.
        let block_index = block.index();
        let catching_up = context.skips_live_observers()
            || (context.uses_dynamic_peer_tip()
                && live_tip > 0
                && u64::from(block_index).saturating_add(COMMITTED_CATCHUP_DISTANCE) < live_tip);
        if catching_up {
            return;
        }

        let application_logs = self.application_logs_service.as_ref().map(Arc::clone);
        let tokens_tracker = self.tokens_tracker();
        if application_logs.is_none() && tokens_tracker.is_none() {
            return;
        }

        if let Some(application_logs) = application_logs {
            application_logs.blockchain_committed_handler(self.network, block);
        }
        if let Some(tokens_tracker) = tokens_tracker {
            tokens_tracker.blockchain_committed_handler(self.network, block);
        }
    }

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
        // StateService and a persistent indexer can make an independent store
        // durable before Ledger. ApplicationLogs and TokensTracker only stage
        // here and commit from the post-canonical callback, so they do not arm
        // the cross-store recovery marker.
        let has_persistent_indexer = !catching_up
            && self
                .indexer_service
                .as_ref()
                .is_some_and(|indexer| indexer.is_persistent());
        if (should_track_state || has_persistent_indexer)
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

        if let Some(indexer) = &self.indexer_service {
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
        }

        self.commit_plugin_committing_handlers(block, snapshot, application_executed_list);
        true
    }

    fn commit_plugin_committing_handlers<B: CacheRead>(
        &self,
        block: &Block,
        snapshot: &DataCache<B>,
        application_executed_list: &[ApplicationExecuted],
    ) {
        let application_logs = self.application_logs_service.as_ref().map(Arc::clone);
        let tokens_tracker = self.tokens_tracker();
        if application_logs.is_none() && tokens_tracker.is_none() {
            return;
        }

        if let Some(application_logs) = application_logs {
            application_logs.blockchain_committing_handler(
                self.network,
                block,
                snapshot,
                application_executed_list,
            );
        }
        if let Some(tokens_tracker) = tokens_tracker {
            tokens_tracker.blockchain_committing_handler(
                self.network,
                block,
                snapshot,
                application_executed_list,
            );
        }
    }
}

impl<P, S, L, T, B> BlockCommitHooks<B> for DaemonCommitHooks<P, S, L, T>
where
    P: NativeContractProvider + 'static,
    S: Store + 'static,
    L: Store + 'static,
    T: Store + 'static,
    B: CacheRead,
{
    fn block_committing(
        &self,
        block: &Block,
        snapshot: &DataCache<B>,
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

    fn block_committed(&self, block: &Block, live_tip: u64, context: BlockPersistContext) {
        self.block_committed_with_live_tip_and_context(block, live_tip, context);
    }

    fn sync_batch_commit_policy(
        &self,
        _start_height: u32,
        end_height: u32,
        live_tip: u64,
    ) -> SyncBatchCommitPolicy {
        let observers_skipped_for_entire_batch = live_tip > 0
            && u64::from(end_height).saturating_add(COMMITTING_CATCHUP_DISTANCE) < live_tip;
        let has_post_canonical_staging =
            self.application_logs_service.is_some() || self.tokens_tracker().is_some();

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
            state_service.flush_durable_result()?;
        }
        if let Some(indexer) = &self.indexer_service {
            indexer
                .flush_durable()
                .map_err(|error| format!("indexer durability fence failed: {error}"))?;
        }
        Ok(())
    }

    fn canonical_commit_succeeded(&self) {
        self.replay_guard.canonical_commit_succeeded();
    }

    fn canonical_commit_failed(&self, reason: &str) {
        self.replay_guard.canonical_commit_failed(reason);
    }

    fn should_stop_blockchain_service(&self) -> bool {
        self.replay_guard.shutdown_requested()
    }

    fn allows_empty_block_fast_forward(&self) -> bool {
        self.state_service.is_none()
            && self.indexer_service.is_none()
            && self.application_logs_service.is_none()
            && self.tokens_tracker().is_none()
    }

    fn allows_empty_block_committing_fast_forward(&self) -> bool {
        self.state_service.is_some()
            && self.indexer_service.is_none()
            && self.application_logs_service.is_none()
            && self.tokens_tracker().is_none()
    }
}
