use std::sync::Arc;

use neo_error::{CoreError, CoreResult};
use neo_payloads::block::Block;

use crate::service::{BlockchainService, MempoolLike};

use super::super::consensus_witness_stage::{
    NeoConsensusWitnessStage, SnapshotConsensusWitnessContext,
};
use super::super::stage_traits::EngineError;
use super::super::verified_import_pipeline::VerifiedImportPipeline;

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

    pub(crate) fn verify_consensus_witness_against_store(&self, block: &Block) -> CoreResult<()> {
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

    pub(crate) fn verify_consensus_witness_against_snapshot_with_native_provider(
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

    pub(crate) async fn verify_import_block_with_pipeline(
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

    pub(crate) fn ensure_block_matches_cached_header(
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

    /// Compute the hash of a block. Returns an error string when the
    /// header cannot be hashed (e.g. because it is missing).
    pub(crate) fn try_block_hash(block: &Block) -> CoreResult<neo_primitives::UInt256> {
        let header = block.header.clone();
        header
            .try_hash()
            .map_err(|err| CoreError::other(format!("hash computation failed: {err}")))
    }
}
