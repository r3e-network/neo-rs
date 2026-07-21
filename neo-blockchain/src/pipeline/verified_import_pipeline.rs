//! Verified import pipeline used by `ImportMode::Sync` and verified live/replay imports.
//!
//! This is the narrow block-import chain used before native persistence for
//! verification-enabled local imports: structural/stateful validation first,
//! then dBFT header witness authorization. It keeps the handler focused on
//! import policy while the pipeline owns the stage order.

use std::fmt;
use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_execution::{PreverifiedSignatureCache, native_contract_provider::NativeContractProvider};
use neo_payloads::Block;
use neo_storage::{CacheRead, DataCache};

use super::consensus_witness_stage::{
    ConsensusWitnessContext, NeoConsensusWitnessStage, SnapshotConsensusWitnessContext,
};
use super::stage_traits::{EngineResult, StageContext, ValidateStage};
use super::validate_stage::{NeoValidateStage, SnapshotValidateContext};

/// Concrete verified-import chain: validate, then verify consensus witness.
pub struct VerifiedImportPipeline<P, B>
where
    P: NativeContractProvider + 'static,
    B: CacheRead,
    SnapshotConsensusWitnessContext<P, B>: ConsensusWitnessContext,
{
    validate: NeoValidateStage<SnapshotValidateContext<B>>,
    consensus_witness: NeoConsensusWitnessStage<SnapshotConsensusWitnessContext<P, B>>,
}

impl<P, B> fmt::Debug for VerifiedImportPipeline<P, B>
where
    P: NativeContractProvider + 'static,
    B: CacheRead,
    SnapshotConsensusWitnessContext<P, B>: ConsensusWitnessContext,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VerifiedImportPipeline")
            .field("validate", &self.validate)
            .field("consensus_witness", &self.consensus_witness)
            .finish()
    }
}

impl<P, B> VerifiedImportPipeline<P, B>
where
    P: NativeContractProvider + 'static,
    B: CacheRead,
    SnapshotConsensusWitnessContext<P, B>: ConsensusWitnessContext,
{
    /// Creates a verified-import chain over one immutable snapshot.
    #[must_use]
    pub fn new(
        settings: Arc<ProtocolSettings>,
        snapshot: Arc<DataCache<B>>,
        native_contract_provider: Arc<P>,
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
    pub fn verify(&self, ctx: &StageContext, block: &Block) -> EngineResult<()> {
        self.verify_with_signature_cache(ctx, block, None)
    }

    /// Runs the complete verified-import chain with optional ECDSA preverification.
    ///
    /// Validation and canonical consensus-witness NeoVM execution always run.
    /// The cache can replace only exact curve operations inside that execution.
    pub fn verify_with_signature_cache(
        &self,
        ctx: &StageContext,
        block: &Block,
        signature_cache: Option<Arc<PreverifiedSignatureCache>>,
    ) -> EngineResult<()> {
        self.validate.validate(ctx, block)?;
        self.consensus_witness
            .verify_block_with_signature_cache(block, signature_cache)
    }

    /// Creates and runs the verified-import chain for one block.
    pub fn verify_block(
        block: &Block,
        current_height: u32,
        trusted_replay: bool,
        settings: Arc<ProtocolSettings>,
        snapshot: Arc<DataCache<B>>,
        native_contract_provider: Arc<P>,
    ) -> EngineResult<()> {
        Self::verify_block_with_signature_cache(
            block,
            current_height,
            trusted_replay,
            settings,
            snapshot,
            native_contract_provider,
            None,
        )
    }

    /// Creates and runs the complete chain with optional ECDSA preverification.
    #[allow(clippy::too_many_arguments)]
    pub fn verify_block_with_signature_cache(
        block: &Block,
        current_height: u32,
        trusted_replay: bool,
        settings: Arc<ProtocolSettings>,
        snapshot: Arc<DataCache<B>>,
        native_contract_provider: Arc<P>,
        signature_cache: Option<Arc<PreverifiedSignatureCache>>,
    ) -> EngineResult<()> {
        let pipeline = Self::new(settings, snapshot, native_contract_provider);
        pipeline.verify_with_signature_cache(
            &StageContext::for_verified_import(current_height, trusted_replay),
            block,
            signature_cache,
        )
    }
}

#[cfg(test)]
#[path = "../tests/pipeline/verified_import_pipeline.rs"]
mod tests;
