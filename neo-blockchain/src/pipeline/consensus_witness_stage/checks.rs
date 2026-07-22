use std::sync::Arc;

use neo_execution::{Helper, PreverifiedSignatureCache};
use neo_payloads::Block;

use crate::block_validation::BlockValidator;
use crate::pipeline::stage_traits::{EngineError, EngineResult};

use super::{CONSENSUS_WITNESS_MAX_GAS, ConsensusWitnessContext, NeoConsensusWitnessStage};

impl<C> NeoConsensusWitnessStage<C>
where
    C: ConsensusWitnessContext,
{
    /// Synchronous verification entry point used by legacy service helpers.
    pub fn verify_block(&self, block: &Block) -> EngineResult<()> {
        self.verify_block_with_signature_cache(block, None)
    }

    /// Verifies one block with optional exact ECDSA preverification outcomes.
    ///
    /// The cache is advisory: parent checks and the complete canonical NeoVM
    /// witness script always execute. A cache miss uses ordinary secp256r1
    /// verification inside the same helper.
    pub fn verify_block_with_signature_cache(
        &self,
        block: &Block,
        signature_cache: Option<Arc<PreverifiedSignatureCache>>,
    ) -> EngineResult<()> {
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

        let chain_spec = self.ctx.chain_spec();
        let settings = chain_spec.protocol_settings();
        BlockValidator::validate_primary_index(block.primary_index(), settings.validators_count)
            .map_err(|error| EngineError::validation_failed(block.index(), error.to_string()))?;
        let verify_result = match signature_cache {
            Some(signature_cache) => {
                Helper::verify_witness_with_native_provider_and_signature_cache(
                    &block.header,
                    settings,
                    self.ctx.snapshot(),
                    &parent.next_consensus,
                    &block.header.witness,
                    CONSENSUS_WITNESS_MAX_GAS,
                    self.ctx.native_contract_provider(),
                    signature_cache,
                )
            }
            None => Helper::verify_witness_with_native_provider(
                &block.header,
                settings,
                self.ctx.snapshot(),
                &parent.next_consensus,
                &block.header.witness,
                CONSENSUS_WITNESS_MAX_GAS,
                self.ctx.native_contract_provider(),
            ),
        };
        verify_result.map_err(|_| {
            EngineError::validation_failed(block.index(), "consensus witness verification failed")
        })?;

        Ok(())
    }
}
