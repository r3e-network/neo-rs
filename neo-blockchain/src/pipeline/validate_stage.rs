//! Concrete `ValidateStage` implementation for the block processing pipeline.
//!
//! This module implements the [`ValidateStage`] trait
//! by wrapping the existing pure [`crate::block_validation::BlockValidator`]
//! checks. It is the first concrete `PipelineStage` implementation in the workspace
//! (ADR-010 Phase 1).
//!
//! # Design
//!
//! The stage is split into two layers:
//! - **Stateless checks** — size, transaction count, merkle root, duplicate
//!   transactions, witness scripts, block version. These delegate to
//!   `BlockValidator` associated functions and require no external state.
//! - **Stateful checks** — timestamp progression, header chaining (prev hash +
//!   height), primary index validation. These require protocol settings and
//!   a store snapshot, injected via [`ValidateContext`].
//!
//! # Wiring Status
//!
//! Verified block import constructs this stage over
//! the same snapshot used by native persistence before running
//! `NeoConsensusWitnessStage`. Live peer inventory still keeps its inline
//! import-integrity checks because that path intentionally follows C#
//! `OnNewBlock` relay semantics and must not inherit consensus-production
//! transaction-count limits; its consensus witness check is routed through the
//! same consensus-witness stage helper.
//!
//! # Bulk-Sync Behavior
//!
//! When `StageContext.trusted_replay` is true, the stage skips timestamp drift
//! checks (trusted local replay path). This matches `ImportMode::TrustedReplay`
//! with `verify: false`; `ImportMode::Sync` always runs full validation.

use std::fmt;
use std::sync::Arc;

mod checks;
mod context;

pub use context::{SnapshotValidateContext, ValidateContext};

use super::stage_traits::{
    EngineResult, PipelineStage, StageContext, StageId, StageOutput, ValidateStage,
};
use neo_payloads::Block;

/// Concrete validate stage wrapping [`crate::block_validation::BlockValidator`]
/// + stateful checks.
///
/// Construct via [`NeoValidateStage::new`].
pub struct NeoValidateStage<C = SnapshotValidateContext<neo_storage::EmptyCacheBacking>>
where
    C: ValidateContext,
{
    ctx: Arc<C>,
}

impl<C> fmt::Debug for NeoValidateStage<C>
where
    C: ValidateContext,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NeoValidateStage")
            .field("validators_count", &self.ctx.validators_count())
            .finish_non_exhaustive()
    }
}

impl<C> NeoValidateStage<C>
where
    C: ValidateContext,
{
    /// Create a new validate stage with the given context.
    pub fn new(ctx: Arc<C>) -> Self {
        Self { ctx }
    }
}

impl<C> ValidateStage for NeoValidateStage<C>
where
    C: ValidateContext,
{
    fn validate(&self, ctx: &StageContext, block: &Block) -> EngineResult<()> {
        let settings = self.ctx.settings();

        // Stateless checks always run.
        Self::run_stateless_checks(block, &settings)
            .map_err(|e| Self::map_validation_error(block, e))?;

        // Stateful checks.
        self.run_stateful_checks(block, ctx)
            .map_err(|e| Self::map_validation_error(block, e))?;

        Ok(())
    }
}

impl<C> PipelineStage for NeoValidateStage<C>
where
    C: ValidateContext,
{
    fn id(&self) -> StageId {
        StageId::Validate
    }

    fn execute(&self, ctx: &StageContext, block: &Block) -> EngineResult<StageOutput> {
        let start = std::time::Instant::now();

        self.validate(ctx, block)?;

        Ok(StageOutput::performed(neo_runtime::time::elapsed_us(
            start.elapsed(),
        )))
    }
}

#[cfg(test)]
#[path = "../tests/pipeline/validate_stage.rs"]
mod tests;
