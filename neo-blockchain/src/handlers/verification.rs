use std::sync::Arc;

use neo_error::{CoreError, CoreResult};
use neo_execution::{PreverifiedSignatureCache, PreverifiedSignatureCacheMetricsSnapshot};
use neo_payloads::block::Block;

use crate::pipeline::consensus_witness_stage::{
    NeoConsensusWitnessStage, SnapshotConsensusWitnessContext,
};
use crate::pipeline::signature_verification::HeaderSignaturePreverification;
use crate::pipeline::stage_traits::EngineError;
use crate::pipeline::verified_import_pipeline::VerifiedImportPipeline;
use crate::service::{BlockchainService, MempoolLike};

impl<S, M> BlockchainService<S, M>
where
    S: crate::service_context::SystemContext,
    M: MempoolLike,
{
    fn pipeline_error(block: &Block, error: EngineError) -> CoreError {
        match error {
            EngineError::ValidationFailed { reason, .. } => {
                CoreError::other(format!("block {}: {reason}", block.index()))
            }
            other => CoreError::other(format!("block {}: {other}", block.index())),
        }
    }

    pub(crate) fn verify_consensus_witness_against_store(
        &self,
        block: &Block,
        signature_preverification: Option<&HeaderSignaturePreverification>,
    ) -> CoreResult<()> {
        let settings = self.system.settings();
        let snapshot = self.system.store_snapshot().ok_or_else(|| {
            CoreError::other(format!(
                "block {}: store snapshot unavailable",
                block.index()
            ))
        })?;
        self.verify_consensus_witness_against_snapshot_with_native_provider(
            block,
            settings,
            snapshot,
            self.system.native_contract_provider().ok_or_else(|| {
                CoreError::other(format!(
                    "block {}: native contract provider unavailable",
                    block.index()
                ))
            })?,
            signature_preverification,
        )
    }

    fn verify_consensus_witness_against_snapshot_with_native_provider(
        &self,
        block: &Block,
        settings: Arc<neo_config::ProtocolSettings>,
        snapshot: Arc<neo_storage::DataCache<S::CacheBacking>>,
        native_contract_provider: Arc<S::NativeProvider>,
        signature_preverification: Option<&HeaderSignaturePreverification>,
    ) -> CoreResult<()> {
        let signature_cache = signature_preverification
            .filter(|proof| proof.matches(&block.header, settings.as_ref()))
            .map(HeaderSignaturePreverification::signature_cache);
        let cache_metrics_before = signature_cache
            .as_ref()
            .map(|cache| cache.metrics_snapshot());
        let stage = NeoConsensusWitnessStage::new(Arc::new(SnapshotConsensusWitnessContext::new(
            settings,
            snapshot,
            native_contract_provider,
        )));
        let result = stage
            .verify_block_with_signature_cache(block, signature_cache.as_ref().map(Arc::clone));
        if let (Some(cache), Some(before)) = (signature_cache.as_ref(), cache_metrics_before) {
            self.record_header_signature_cache_consumption(cache, before);
        }
        result.map_err(|error| Self::pipeline_error(block, error))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn verify_import_block_with_pipeline(
        &self,
        block: &Block,
        current_height: u32,
        trusted_replay: bool,
        settings: Arc<neo_config::ProtocolSettings>,
        snapshot: Arc<neo_storage::DataCache<S::CacheBacking>>,
        native_contract_provider: Arc<S::NativeProvider>,
        signature_preverification: Option<&HeaderSignaturePreverification>,
    ) -> CoreResult<()> {
        let signature_cache = signature_preverification
            .filter(|proof| proof.matches(&block.header, settings.as_ref()))
            .map(HeaderSignaturePreverification::signature_cache);
        let cache_metrics_before = signature_cache
            .as_ref()
            .map(|cache| cache.metrics_snapshot());
        let result = VerifiedImportPipeline::<S::NativeProvider, S::CacheBacking>::verify_block_with_signature_cache(
            block,
            current_height,
            trusted_replay,
            settings,
            snapshot,
            native_contract_provider,
            signature_cache.as_ref().map(Arc::clone),
        );
        if let (Some(cache), Some(before)) = (signature_cache.as_ref(), cache_metrics_before) {
            self.record_header_signature_cache_consumption(cache, before);
        }
        result.map_err(|error| Self::pipeline_error(block, error))
    }

    pub(crate) fn record_header_signature_cache_consumption(
        &self,
        cache: &PreverifiedSignatureCache,
        before: PreverifiedSignatureCacheMetricsSnapshot,
    ) {
        if let Some(pool) = self.optimistic_signature_verification.as_ref() {
            pool.record_header_cache_consumption(cache, before);
        }
    }

    pub(crate) fn ensure_block_matches_cached_header(
        &self,
        index: u32,
        hash: neo_primitives::UInt256,
    ) -> CoreResult<()> {
        if let Some(cached_hash) = self.header_cache.hash_at(index) {
            if cached_hash != hash {
                return Err(CoreError::other(format!(
                    "block {index}: hash does not match cached header"
                )));
            }
        }
        Ok(())
    }

    /// Compute the hash of a block. Returns an error string when the
    /// header cannot be hashed (e.g. because it is missing).
    pub(crate) fn try_block_hash(block: &Block) -> CoreResult<neo_primitives::UInt256> {
        let header = block.header.clone();
        header
            .try_hash()
            .map_err(|err| CoreError::other(format!("hash computation failed: {err}")))
    }
}
