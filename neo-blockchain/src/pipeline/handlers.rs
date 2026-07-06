//! Service method handlers for [`crate::service::BlockchainService`].
//!
//! Each command variant has a corresponding `async fn` method on the
//! service. The dispatch loop in [`crate::service::BlockchainService::dispatch`]
//! just `match`es on the command enum and calls the right method. The service
//! stays explicit: no dynamic downcasting and no per-message trait machinery.
//!
//! The handlers own the service-side Neo protocol decisions: block/header
//! sequencing, native persistence, transaction admission, extensible payload
//! verification, and cache maintenance.

use std::sync::Arc;

use neo_error::{CoreError, CoreResult};
use neo_payloads::block::Block;

use crate::relay_result::RelayResult;
use crate::service::{BlockchainService, MempoolLike};

use super::consensus_witness_stage::{NeoConsensusWitnessStage, SnapshotConsensusWitnessContext};
use super::stage_traits::EngineError;
use super::verified_import_pipeline::VerifiedImportPipeline;

#[path = "../handlers/block_inventory.rs"]
mod block_inventory;
#[path = "../handlers/empty_fast_forward.rs"]
mod empty_fast_forward;
#[path = "../handlers/extensible.rs"]
mod extensible;
#[path = "../handlers/headers.rs"]
mod headers;
#[path = "../handlers/import.rs"]
mod import;
#[path = "../handlers/initialize.rs"]
mod initialize;
#[path = "../handlers/persist_completed.rs"]
mod persist_completed;
#[path = "../handlers/reverify.rs"]
mod reverify;
#[path = "../handlers/transactions.rs"]
mod transactions;

impl<S, M> BlockchainService<S, M>
where
    S: crate::service_context::SystemContext,
    M: MempoolLike,
{
    fn pipeline_error(block: &Block, error: EngineError) -> CoreError {
        match error {
            EngineError::ValidationFailed { reason, .. } => {
                CoreError::other(format!("block {}: {reason}", block.index()))
            }
            other => CoreError::other(format!("block {}: {other}", block.index())),
        }
    }

    fn verify_consensus_witness_against_store(&self, block: &Block) -> CoreResult<()> {
        let settings = self.system.settings();
        let snapshot = self.system.store_snapshot().ok_or_else(|| {
            CoreError::other(format!(
                "block {}: store snapshot unavailable",
                block.index()
            ))
        })?;
        self.verify_consensus_witness_against_snapshot_with_native_provider(
            block,
            settings,
            snapshot,
            self.system.native_contract_provider(),
        )
    }

    fn verify_consensus_witness_against_snapshot_with_native_provider(
        &self,
        block: &Block,
        settings: Arc<neo_config::ProtocolSettings>,
        snapshot: Arc<neo_storage::DataCache>,
        native_contract_provider: Option<
            Arc<dyn neo_execution::native_contract_provider::NativeContractProvider>,
        >,
    ) -> CoreResult<()> {
        let stage = NeoConsensusWitnessStage::new(Arc::new(SnapshotConsensusWitnessContext::new(
            settings,
            snapshot,
            native_contract_provider,
        )));
        stage
            .verify_block(block)
            .map_err(|error| Self::pipeline_error(block, error))
    }

    async fn verify_import_block_with_pipeline(
        &self,
        block: &Block,
        current_height: u32,
        bulk_sync: bool,
        settings: Arc<neo_config::ProtocolSettings>,
        snapshot: Arc<neo_storage::DataCache>,
        native_contract_provider: Option<
            Arc<dyn neo_execution::native_contract_provider::NativeContractProvider>,
        >,
    ) -> CoreResult<()> {
        VerifiedImportPipeline::verify_block(
            block,
            current_height,
            bulk_sync,
            settings,
            snapshot,
            native_contract_provider,
        )
        .await
        .map_err(|error| Self::pipeline_error(block, error))
    }

    fn ensure_block_matches_cached_header(
        &self,
        index: u32,
        hash: neo_primitives::UInt256,
    ) -> CoreResult<()> {
        if let Some(cached_header) = self.header_cache.get(index) {
            let cached_hash = cached_header.hash();
            if cached_hash != hash {
                return Err(CoreError::other(format!(
                    "block {index}: hash does not match cached header"
                )));
            }
        }
        Ok(())
    }

    /// Handle a [`BlockchainCommand::RelayResult`] notification.
    pub(crate) async fn handle_relay_result(&self, _result: RelayResult) {}

    /// Compute the hash of a block. Returns an error string when the
    /// header cannot be hashed (e.g. because it is missing).
    pub(crate) fn try_block_hash(block: &Block) -> CoreResult<neo_primitives::UInt256> {
        let header = block.header.clone();
        header
            .try_hash()
            .map_err(|err| CoreError::other(format!("hash computation failed: {err}")))
    }
}

// =============================================================================
// Tests
// =============================================================================
#[cfg(test)]
#[path = "../tests/pipeline/handlers.rs"]
mod tests;
