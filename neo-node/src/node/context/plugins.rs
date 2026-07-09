//! Read-side plugin and deferred commit-hook dispatch for `DaemonContext`.
//!
//! These hooks are node-local orchestration. They decide when to run expensive
//! StateService, indexer, ApplicationLogs, and TokensTracker work around bulk
//! sync, but do not define the underlying protocol or storage semantics.

use std::sync::Arc;

use neo_blockchain::service_context::BlockPersistContext;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_payloads::{ApplicationExecuted, Block, CommittedHandler, CommittingHandler};
use neo_storage::persistence::DataCache;
use tracing::warn;

use super::DaemonContext;

impl<P> DaemonContext<P>
where
    P: NativeContractProvider + 'static,
{
    pub(in crate::node) fn block_committed_with_live_tip_and_context(
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
        let catching_up =
            context.bulk_sync || (live_tip > 0 && (block_index as u64) + 1000 < live_tip);
        if catching_up {
            return;
        }

        let application_logs = self.application_logs_service.as_ref().map(Arc::clone);
        let tokens_tracker = self.tokens_tracker();
        if application_logs.is_none() && tokens_tracker.is_none() {
            return;
        }

        if let Some(application_logs) = application_logs {
            application_logs.blockchain_committed_handler(self.settings.as_ref(), block);
        }
        if let Some(tokens_tracker) = tokens_tracker {
            tokens_tracker.blockchain_committed_handler(self.settings.as_ref(), block);
        }
    }

    pub(in crate::node) fn block_committing_with_live_tip(
        &self,
        block: &Block,
        snapshot: &DataCache,
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

    pub(in crate::node) fn block_committing_with_live_tip_and_context(
        &self,
        block: &Block,
        snapshot: &DataCache,
        application_executed_list: &[ApplicationExecuted],
        live_tip: u64,
        context: BlockPersistContext,
    ) -> bool {
        // During catch-up, skip the expensive per-block hooks:
        // - StateService.on_committing computes the MPT state root per block
        //   (~24ms measured — the dominant sync bottleneck). Validation
        //   profiles can force it on with [state_service].track_during_catchup.
        // - IndexerService.index_block indexes transaction execution results.
        // Deferred hooks resume near the live tip. This mirrors C# Neo's chain.acc
        // import which skips verification and indexing during bulk sync.
        let block_index = block.index();
        let catching_up =
            context.bulk_sync || (live_tip > 0 && (block_index as u64) + 10000 < live_tip);

        if let Some(state_service) = &self.state_service {
            let should_track_state = !catching_up || self.state_service_track_during_catchup;
            let state_ok = if should_track_state && catching_up {
                state_service.on_committing_deferred(block.index(), snapshot)
            } else if should_track_state {
                state_service.on_committing(block.index(), snapshot)
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
            && let Err(error) =
                indexer.index_block_with_application_executions(block, application_executed_list)
        {
            warn!(
                target: "neo::indexer",
                height = block.index(),
                error = %error,
                "failed to index block application executions"
            );
        }

        self.commit_plugin_committing_handlers(block, snapshot, application_executed_list);
        true
    }

    fn commit_plugin_committing_handlers(
        &self,
        block: &Block,
        snapshot: &DataCache,
        application_executed_list: &[ApplicationExecuted],
    ) {
        let application_logs = self.application_logs_service.as_ref().map(Arc::clone);
        let tokens_tracker = self.tokens_tracker();
        if application_logs.is_none() && tokens_tracker.is_none() {
            return;
        }

        if let Some(application_logs) = application_logs {
            application_logs.blockchain_committing_handler(
                self.settings.as_ref(),
                block,
                snapshot,
                application_executed_list,
            );
        }
        if let Some(tokens_tracker) = tokens_tracker {
            tokens_tracker.blockchain_committing_handler(
                self.settings.as_ref(),
                block,
                snapshot,
                application_executed_list,
            );
        }
    }
}
