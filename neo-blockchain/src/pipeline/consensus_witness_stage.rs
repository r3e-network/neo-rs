//! Consensus-witness verification stage for block import.
//!
//! `NeoValidateStage` owns structural block validation and stateful parent
//! checks. This stage owns the consensus-specific authorization check: load the
//! previous block, use its `NextConsensus` account, and verify the incoming
//! header witness through the NeoVM helper with the caller's explicit native
//! contract provider.

use std::fmt;
use std::sync::Arc;

mod checks;
mod context;

pub use context::{ConsensusWitnessContext, ParentHeaderContext, SnapshotConsensusWitnessContext};

use async_trait::async_trait;
use neo_payloads::Block;

use super::stage_traits::{
    ConsensusWitnessStage, EngineResult, PipelineStage, StageContext, StageId, StageOutput,
};

/// GAS limit used for consensus header witness verification.
///
/// This is the same budget used by the previous inline verifier and mirrors the
/// C# header verification path.
pub const CONSENSUS_WITNESS_MAX_GAS: i64 = 300_000_000;

/// Concrete consensus-witness verification stage.
pub struct NeoConsensusWitnessStage<C = SnapshotConsensusWitnessContext>
where
    C: ConsensusWitnessContext,
{
    ctx: Arc<C>,
}

impl<C> fmt::Debug for NeoConsensusWitnessStage<C>
where
    C: ConsensusWitnessContext,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NeoConsensusWitnessStage")
            .field("ctx", &self.ctx)
            .finish_non_exhaustive()
    }
}

impl<C> NeoConsensusWitnessStage<C>
where
    C: ConsensusWitnessContext,
{
    /// Creates a new consensus-witness stage.
    #[must_use]
    pub fn new(ctx: Arc<C>) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl<C> ConsensusWitnessStage for NeoConsensusWitnessStage<C>
where
    C: ConsensusWitnessContext,
{
    async fn verify_consensus_witness(
        &self,
        _ctx: &StageContext,
        block: &Block,
    ) -> EngineResult<()> {
        self.verify_block(block)
    }
}

#[async_trait]
impl<C> PipelineStage for NeoConsensusWitnessStage<C>
where
    C: ConsensusWitnessContext,
{
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
