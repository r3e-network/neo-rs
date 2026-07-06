//! Consensus-witness verification stage for block import.
//!
//! `NeoValidateStage` owns structural block validation and stateful parent
//! checks. This stage owns the consensus-specific authorization check: load the
//! previous block, use its `NextConsensus` account, and verify the incoming
//! header witness through the NeoVM helper with the caller's explicit native
//! contract provider.

use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use neo_config::ProtocolSettings;
use neo_error::{CoreError, CoreResult};
use neo_execution::Helper;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_native_contracts::LedgerContract;
use neo_payloads::Block;
use neo_primitives::{UInt160, UInt256};
use neo_storage::DataCache;

use super::stage_traits::{
    ConsensusWitnessStage, EngineError, EngineResult, PipelineStage, StageContext, StageId,
    StageOutput,
};

/// GAS limit used for consensus header witness verification.
///
/// This is the same budget used by the previous inline verifier and mirrors the
/// C# header verification path.
pub const CONSENSUS_WITNESS_MAX_GAS: i64 = 300_000_000;

/// Parent-header data needed to verify a child block's consensus witness.
#[derive(Debug, Clone, Copy)]
pub struct ParentHeaderContext {
    /// Hash of the parent block.
    pub hash: UInt256,
    /// Height of the parent block.
    pub index: u32,
    /// Parent timestamp in milliseconds.
    pub timestamp: u64,
    /// Parent `NextConsensus`, the account that must authorize this header.
    pub next_consensus: UInt160,
}

/// Narrow context required by [`NeoConsensusWitnessStage`].
pub trait ConsensusWitnessContext: Send + Sync + fmt::Debug + 'static {
    /// Returns the protocol settings used by witness verification.
    fn settings(&self) -> Arc<ProtocolSettings>;

    /// Returns the snapshot used for contract lookups during verification.
    fn snapshot(&self) -> &DataCache;

    /// Returns the explicit native provider used by NeoVM host calls.
    fn native_contract_provider(&self) -> Option<Arc<dyn NativeContractProvider>>;

    /// Resolves the previous header context for `block`.
    fn parent_header(&self, block: &Block) -> CoreResult<ParentHeaderContext>;
}

/// Snapshot-backed consensus-witness context used by service handlers.
#[derive(Clone)]
pub struct SnapshotConsensusWitnessContext {
    settings: Arc<ProtocolSettings>,
    snapshot: Arc<DataCache>,
    native_contract_provider: Option<Arc<dyn NativeContractProvider>>,
}

impl fmt::Debug for SnapshotConsensusWitnessContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SnapshotConsensusWitnessContext")
            .field("validators_count", &self.settings.validators_count)
            .field(
                "has_native_contract_provider",
                &self.native_contract_provider.is_some(),
            )
            .finish_non_exhaustive()
    }
}

impl SnapshotConsensusWitnessContext {
    /// Creates a context over an immutable store snapshot and explicit native
    /// provider.
    #[must_use]
    pub fn new(
        settings: Arc<ProtocolSettings>,
        snapshot: Arc<DataCache>,
        native_contract_provider: Option<Arc<dyn NativeContractProvider>>,
    ) -> Self {
        Self {
            settings,
            snapshot,
            native_contract_provider,
        }
    }
}

impl ConsensusWitnessContext for SnapshotConsensusWitnessContext {
    fn settings(&self) -> Arc<ProtocolSettings> {
        Arc::clone(&self.settings)
    }

    fn snapshot(&self) -> &DataCache {
        self.snapshot.as_ref()
    }

    fn native_contract_provider(&self) -> Option<Arc<dyn NativeContractProvider>> {
        self.native_contract_provider.clone()
    }

    fn parent_header(&self, block: &Block) -> CoreResult<ParentHeaderContext> {
        let prev = LedgerContract::new()
            .get_trimmed_block(self.snapshot.as_ref(), block.header.prev_hash())?
            .ok_or_else(|| CoreError::other("previous block not found"))?;

        Ok(ParentHeaderContext {
            hash: prev.hash(),
            index: prev.index(),
            timestamp: prev.header.timestamp(),
            next_consensus: *prev.header.next_consensus(),
        })
    }
}

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
