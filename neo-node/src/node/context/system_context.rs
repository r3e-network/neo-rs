//! Blockchain service context implementation for the daemon.
//!
//! This file adapts `DaemonContext` to the protocol-facing
//! `SystemContext` trait. It delegates expensive read-side hooks to
//! `plugins.rs` so this layer only expresses the trait contract and store
//! commit policy.

use std::sync::Arc;

use neo_blockchain::service_context::{BlockPersistContext, SystemContext};
use neo_config::ProtocolSettings;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_payloads::{ApplicationExecuted, Block};
use neo_storage::persistence::DataCache;

use crate::node::ledger_source::snapshot_ledger_index;

use super::DaemonContext;

impl<P> SystemContext for DaemonContext<P>
where
    P: NativeContractProvider + 'static,
{
    fn settings(&self) -> Arc<ProtocolSettings> {
        Arc::clone(&self.settings)
    }

    fn current_height(&self) -> u32 {
        snapshot_ledger_index(&self.snapshot).unwrap_or(0)
    }

    fn store_snapshot(&self) -> Option<Arc<DataCache>> {
        Some(Arc::clone(&self.snapshot))
    }

    fn native_contract_provider(&self) -> Option<Arc<dyn NativeContractProvider>> {
        let provider: Arc<dyn NativeContractProvider> = self.native_contract_provider.clone();
        Some(provider)
    }

    fn block_committing(
        &self,
        block: &Block,
        snapshot: &DataCache,
        application_executed_list: &[ApplicationExecuted],
    ) -> bool {
        self.block_committing_with_live_tip(
            block,
            snapshot,
            application_executed_list,
            neo_runtime::sync_metrics::peer_live_tip(),
        )
    }

    fn block_committing_with_context(
        &self,
        block: &Block,
        snapshot: &DataCache,
        application_executed_list: &[ApplicationExecuted],
        context: BlockPersistContext,
    ) -> bool {
        self.block_committing_with_live_tip_and_context(
            block,
            snapshot,
            application_executed_list,
            neo_runtime::sync_metrics::peer_live_tip(),
            context,
        )
    }

    fn block_committed(&self, block: &Block) {
        self.block_committed_with_live_tip_and_context(
            block,
            neo_runtime::sync_metrics::peer_live_tip(),
            BlockPersistContext::live(),
        );
    }

    fn block_committed_with_context(&self, block: &Block, context: BlockPersistContext) {
        self.block_committed_with_live_tip_and_context(
            block,
            neo_runtime::sync_metrics::peer_live_tip(),
            context,
        );
    }

    fn commit_to_store(&self) {
        // The StoreCache's DataCache shares state with `snapshot` (it was cloned
        // from it), so its tracked block writes are flushed through to the store.
        self.store_cache.lock().commit();
    }

    fn flush_bulk_sync_commit_handlers(&self) -> Result<(), String> {
        if let Some(state_service) = &self.state_service {
            state_service
                .flush_result()
                .map_err(|err| err.to_string())?;
        }
        Ok(())
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
