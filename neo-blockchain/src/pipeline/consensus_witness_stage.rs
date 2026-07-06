//! Consensus-witness verification stage for block import.
//!
//! `NeoValidateStage` owns structural block validation and stateful parent
//! checks. This stage owns the consensus-specific authorization check: load the
//! previous block, use its `NextConsensus` account, and verify the incoming
//! header witness through the NeoVM helper with the caller's explicit native
//! contract provider.

use std::fmt;
use std::sync::Arc;

mod context;

pub use context::{ConsensusWitnessContext, ParentHeaderContext, SnapshotConsensusWitnessContext};

use async_trait::async_trait;
use neo_execution::Helper;
use neo_payloads::Block;

use super::stage_traits::{
    ConsensusWitnessStage, EngineError, EngineResult, PipelineStage, StageContext, StageId,
    StageOutput,
};

/// GAS limit used for consensus header witness verification.
///
/// This is the same budget used by the previous inline verifier and mirrors the
/// C# header verification path.
pub const CONSENSUS_WITNESS_MAX_GAS: i64 = 300_000_000;

/// Concrete consensus-witness verification stage.
pub struct NeoConsensusWitnessStage {
    ctx: Arc<dyn ConsensusWitnessContext>,
}

impl fmt::Debug for NeoConsensusWitnessStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NeoConsensusWitnessStage")
            .field("ctx", &self.ctx)
            .finish_non_exhaustive()
    }
}

impl NeoConsensusWitnessStage {
    /// Creates a new consensus-witness stage.
    #[must_use]
    pub fn new(ctx: Arc<dyn ConsensusWitnessContext>) -> Self {
        Self { ctx }
    }

    /// Synchronous verification entry point used by legacy service helpers.
    pub fn verify_block(&self, block: &Block) -> EngineResult<()> {
        let parent = self
            .ctx
            .parent_header(block)
            .map_err(|error| EngineError::validation_failed(block.index(), error.to_string()))?;
        let expected_index = parent.index.checked_add(1).ok_or_else(|| {
            EngineError::validation_failed(block.index(), "previous block index overflow")
        })?;

        if expected_index != block.index() {
            return Err(EngineError::validation_failed(
                block.index(),
                "previous block index mismatch",
            ));
        }

        if parent.hash != *block.header.prev_hash() {
            return Err(EngineError::validation_failed(
                block.index(),
                "previous block hash mismatch",
            ));
        }

        if block.header.timestamp() <= parent.timestamp {
            return Err(EngineError::validation_failed(
                block.index(),
                "timestamp not after previous block",
            ));
        }

        let settings = self.ctx.settings();
        Helper::verify_witness_with_native_provider(
            &block.header,
            settings.as_ref(),
            self.ctx.snapshot(),
            &parent.next_consensus,
            &block.header.witness,
            CONSENSUS_WITNESS_MAX_GAS,
            self.ctx.native_contract_provider(),
        )
        .map_err(|_| {
            EngineError::validation_failed(block.index(), "consensus witness verification failed")
        })?;

        Ok(())
    }
}

#[async_trait]
impl ConsensusWitnessStage for NeoConsensusWitnessStage {
    async fn verify_consensus_witness(
        &self,
        _ctx: &StageContext,
        block: &Block,
    ) -> EngineResult<()> {
        self.verify_block(block)
    }
}

#[async_trait]
impl PipelineStage for NeoConsensusWitnessStage {
    fn id(&self) -> StageId {
        StageId::ConsensusWitness
    }

    async fn execute(&self, ctx: &StageContext, block: &Block) -> EngineResult<StageOutput> {
        let start = std::time::Instant::now();

        self.verify_consensus_witness(ctx, block).await?;

        Ok(StageOutput::performed(neo_runtime::time::elapsed_us(
            start.elapsed(),
        )))
    }
}

#[cfg(test)]
#[path = "../tests/pipeline/consensus_witness_stage.rs"]
mod tests;
