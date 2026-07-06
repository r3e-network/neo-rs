//! Concrete `ValidateStage` implementation for the block processing pipeline.
//!
//! This module implements the [`ValidateStage`] trait
//! by wrapping the existing pure [`BlockValidator`]
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
//! Verified block import (`Import { verify: true }`) constructs this stage over
//! the same snapshot used by native persistence before running
//! `NeoConsensusWitnessStage`. Live peer inventory still keeps its inline
//! import-integrity checks because that path intentionally follows C#
//! `OnNewBlock` relay semantics and must not inherit consensus-production
//! transaction-count limits; its consensus witness check is routed through the
//! same consensus-witness stage helper.
//!
//! # Bulk-Sync Behavior
//!
//! When `StageContext.bulk_sync` is true, the stage skips timestamp drift
//! checks (trusted bulk import path). This matches the existing behavior where
//! `handle_import` with `verify: false` skips stateful header verification.

use std::fmt;
use std::sync::Arc;

mod context;

pub use context::{SnapshotValidateContext, ValidateContext};

use super::stage_traits::{
    EngineError, EngineResult, PipelineStage, StageContext, StageId, StageOutput, ValidateStage,
};
use async_trait::async_trait;
use neo_config::ProtocolSettings;
use neo_payloads::Block;

use super::block_validation::{BlockValidationError, BlockValidator, MIN_TIMESTAMP_MS};

/// Concrete validate stage wrapping [`BlockValidator`] + stateful checks.
///
/// Construct via [`NeoValidateStage::new`].
pub struct NeoValidateStage {
    ctx: Arc<dyn ValidateContext>,
}

impl fmt::Debug for NeoValidateStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NeoValidateStage")
            .field("validators_count", &self.ctx.validators_count())
            .finish_non_exhaustive()
    }
}

impl NeoValidateStage {
    /// Create a new validate stage with the given context.
    pub fn new(ctx: Arc<dyn ValidateContext>) -> Self {
        Self { ctx }
    }

    /// Run all stateless checks (no external state needed).
    fn run_stateless_checks(
        block: &Block,
        settings: &ProtocolSettings,
    ) -> Result<(), BlockValidationError> {
        BlockValidator::validate_block_version(block.version())?;
        BlockValidator::validate_block_size(block)?;
        BlockValidator::validate_transaction_count_raw_with_limit(
            block.transactions.len(),
            settings.max_transactions_per_block as usize,
        )?;

        let tx_hashes = block.transaction_hashes().map_err(|err| {
            BlockValidationError::HeaderValidationFailed {
                reason: format!("failed to hash block transactions: {err}"),
            }
        })?;
        BlockValidator::validate_merkle_root(block.merkle_root(), &tx_hashes)?;
        BlockValidator::validate_no_duplicate_transactions(&tx_hashes)?;

        // Header witness validation.
        BlockValidator::validate_witness_scripts(block.witness())?;

        // Transaction witness script validation.
        for tx in block.transactions.iter() {
            for witness in tx.witnesses() {
                BlockValidator::validate_witness_scripts(witness)?;
            }
        }

        Ok(())
    }

    /// Run stateful checks that require protocol settings and store access.
    fn run_stateful_checks(
        &self,
        block: &Block,
        ctx: &StageContext,
    ) -> Result<(), BlockValidationError> {
        // Primary index validation
        BlockValidator::validate_primary_index(block.primary_index(), self.ctx.validators_count())?;

        // Timestamp bounds
        if !ctx.bulk_sync {
            // Normal mode: check both minimum and future drift.
            BlockValidator::validate_timestamp_bounds(block.timestamp())?;
        } else {
            // Bulk sync: only check the minimum (genesis) timestamp.
            if block.timestamp() < MIN_TIMESTAMP_MS {
                return Err(BlockValidationError::TimestampTooOld {
                    timestamp: block.timestamp(),
                    min: MIN_TIMESTAMP_MS,
                });
            }
        }

        // Timestamp progression (requires prev block)
        if let Some(prev_timestamp) = self.ctx.prev_block_timestamp(ctx.current_height) {
            BlockValidator::validate_timestamp_progression(block.timestamp(), prev_timestamp)?;
        }

        // Header chaining: prev_hash must match the stored hash at current_height
        if let Some(prev_hash) = self.ctx.prev_block_hash(ctx.current_height) {
            if *block.prev_hash() != prev_hash {
                return Err(BlockValidationError::HeaderValidationFailed {
                    reason: format!(
                        "previous hash mismatch: expected {prev_hash}, got {}",
                        block.prev_hash()
                    ),
                });
            }
        }

        // Height sequencing
        let expected_height = ctx.current_height + 1;
        if block.index() != expected_height {
            return Err(BlockValidationError::HeaderValidationFailed {
                reason: format!(
                    "height mismatch: expected {expected_height}, got {}",
                    block.index()
                ),
            });
        }

        Ok(())
    }

    /// Map a `BlockValidationError` to an `EngineError` at the block's height.
    fn map_validation_error(block: &Block, err: BlockValidationError) -> EngineError {
        EngineError::validation_failed(block.header.index(), err.to_string())
    }
}

#[async_trait]
impl ValidateStage for NeoValidateStage {
    async fn validate(&self, ctx: &StageContext, block: &Block) -> EngineResult<()> {
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

#[async_trait]
impl PipelineStage for NeoValidateStage {
    fn id(&self) -> StageId {
        StageId::Validate
    }

    async fn execute(&self, ctx: &StageContext, block: &Block) -> EngineResult<StageOutput> {
        let start = std::time::Instant::now();

        self.validate(ctx, block).await?;

        Ok(StageOutput::performed(neo_runtime::time::elapsed_us(
            start.elapsed(),
        )))
    }
}

#[cfg(test)]
#[path = "../tests/pipeline/validate_stage.rs"]
mod tests;
