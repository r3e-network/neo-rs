use neo_execution::Helper;
use neo_payloads::Block;

use crate::pipeline::stage_traits::{EngineError, EngineResult};

use super::{CONSENSUS_WITNESS_MAX_GAS, NeoConsensusWitnessStage};

impl NeoConsensusWitnessStage {
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
