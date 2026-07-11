use neo_config::ProtocolSettings;
use neo_payloads::Block;

use crate::pipeline::block_validation::{BlockValidationError, BlockValidator, MIN_TIMESTAMP_MS};
use crate::pipeline::stage_traits::{EngineError, StageContext};

use super::{NeoValidateStage, ValidateContext};

impl<C> NeoValidateStage<C>
where
    C: ValidateContext,
{
    /// Run all stateless checks (no external state needed).
    pub(super) fn run_stateless_checks(
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
    pub(super) fn run_stateful_checks(
        &self,
        block: &Block,
        ctx: &StageContext,
    ) -> Result<(), BlockValidationError> {
        // Primary index validation
        BlockValidator::validate_primary_index(block.primary_index(), self.ctx.validators_count())?;

        // Timestamp bounds
        if !ctx.trusted_replay {
            // Normal mode: check both minimum and future drift.
            BlockValidator::validate_timestamp_bounds(block.timestamp())?;
        } else {
            // Trusted replay: only check the minimum (genesis) timestamp.
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
    pub(super) fn map_validation_error(block: &Block, err: BlockValidationError) -> EngineError {
        EngineError::validation_failed(block.header.index(), err.to_string())
    }
}
