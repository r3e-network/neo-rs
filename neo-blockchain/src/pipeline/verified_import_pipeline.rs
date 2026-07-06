//! Verified import pipeline for `Import { verify: true }`.
//!
//! This is the narrow block-import chain used before native persistence for
//! verification-enabled local imports: structural/stateful validation first,
//! then dBFT header witness authorization. It keeps the handler focused on
//! import policy while the pipeline owns the stage order.

use std::fmt;
use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_payloads::Block;
use neo_storage::DataCache;

use super::consensus_witness_stage::{NeoConsensusWitnessStage, SnapshotConsensusWitnessContext};
use super::stage_traits::{ConsensusWitnessStage, EngineResult, StageContext, ValidateStage};
use super::validate_stage::{NeoValidateStage, SnapshotValidateContext};

/// Concrete verified-import chain: validate, then verify consensus witness.
pub struct VerifiedImportPipeline {
    validate: NeoValidateStage,
    consensus_witness: NeoConsensusWitnessStage,
}

impl fmt::Debug for VerifiedImportPipeline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VerifiedImportPipeline")
            .field("validate", &self.validate)
            .field("consensus_witness", &self.consensus_witness)
            .finish()
    }
}

impl VerifiedImportPipeline {
    /// Creates a verified-import chain over one immutable snapshot.
    #[must_use]
    pub fn new(
        settings: Arc<ProtocolSettings>,
        snapshot: Arc<DataCache>,
        native_contract_provider: Option<Arc<dyn NativeContractProvider>>,
    ) -> Self {
        let validate = NeoValidateStage::new(Arc::new(SnapshotValidateContext::new(
            Arc::clone(&settings),
            Arc::clone(&snapshot),
        )));
        let consensus_witness = NeoConsensusWitnessStage::new(Arc::new(
            SnapshotConsensusWitnessContext::new(settings, snapshot, native_contract_provider),
        ));

        Self {
            validate,
            consensus_witness,
        }
    }

    /// Runs the verified-import chain for one block.
    pub async fn verify(&self, ctx: &StageContext, block: &Block) -> EngineResult<()> {
        self.validate.validate(ctx, block).await?;
        self.consensus_witness
            .verify_consensus_witness(ctx, block)
            .await
    }

    /// Creates and runs the verified-import chain for one block.
    pub async fn verify_block(
        block: &Block,
        current_height: u32,
        bulk_sync: bool,
        settings: Arc<ProtocolSettings>,
        snapshot: Arc<DataCache>,
        native_contract_provider: Option<Arc<dyn NativeContractProvider>>,
    ) -> EngineResult<()> {
        let pipeline = Self::new(settings, snapshot, native_contract_provider);
        pipeline
            .verify(
                &StageContext::for_verified_import(current_height, bulk_sync),
                block,
            )
            .await
    }
}

#[cfg(test)]
#[path = "../tests/pipeline/verified_import_pipeline.rs"]
mod tests;
