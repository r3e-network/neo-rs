//! Native-contract read capabilities for consensus orchestration.
//!
//! Consensus needs a narrow NEO/Policy view to configure dBFT validators,
//! `NextConsensus`, block timing, and traceable-conflict windows. Keeping those
//! reads behind a local provider seam makes the node driver depend on
//! capabilities instead of constructing native contracts directly in the
//! consensus flow.

use neo_config::ProtocolSettings;
use neo_crypto::ECPoint;
use neo_native_contracts::{NeoToken, PolicyContract};
use neo_primitives::UInt160;
use neo_storage::persistence::DataCache;

/// Native-contract capabilities required by consensus orchestration.
pub(super) trait ConsensusNativeProvider {
    /// Returns the validators for the next block, in C# consensus order.
    fn next_block_validators(
        &self,
        snapshot: &DataCache,
        validators_count: usize,
    ) -> anyhow::Result<Vec<ECPoint>>;

    /// Returns the `NextConsensus` account for `block_index`.
    fn next_consensus_address_for_block(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
        block_index: u32,
    ) -> anyhow::Result<UInt160>;

    /// Returns the live milliseconds-per-block policy for the round.
    fn milliseconds_per_block(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> anyhow::Result<u32>;

    /// Returns the active `MaxTraceableBlocks` value.
    fn max_traceable_blocks(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> anyhow::Result<u32>;
}

/// Factory for consensus native-contract providers.
pub(super) trait ConsensusNativeProviderFactory {
    /// Provider returned by this factory.
    type Provider: ConsensusNativeProvider;

    /// Creates a provider instance.
    fn provider(&self) -> Self::Provider;
}

/// Production provider backed by canonical native-contract handles.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeConsensusProvider {
    neo: NeoToken,
    policy: PolicyContract,
}

impl NativeConsensusProvider {
    /// Creates a provider backed by canonical native-contract handles.
    #[must_use]
    pub(super) const fn new() -> Self {
        Self {
            neo: NeoToken::new(),
            policy: PolicyContract::new(),
        }
    }
}

impl ConsensusNativeProvider for NativeConsensusProvider {
    fn next_block_validators(
        &self,
        snapshot: &DataCache,
        validators_count: usize,
    ) -> anyhow::Result<Vec<ECPoint>> {
        Ok(self.neo.next_block_validators(snapshot, validators_count)?)
    }

    fn next_consensus_address_for_block(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
        block_index: u32,
    ) -> anyhow::Result<UInt160> {
        Ok(self
            .neo
            .next_consensus_address_for_block(snapshot, settings, block_index)?)
    }

    fn milliseconds_per_block(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> anyhow::Result<u32> {
        Ok(self
            .policy
            .get_milliseconds_per_block_snapshot(snapshot, settings)?)
    }

    fn max_traceable_blocks(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> anyhow::Result<u32> {
        Ok(self
            .policy
            .get_max_traceable_blocks_snapshot(snapshot, settings)?)
    }
}

/// Factory for production consensus native-contract read providers.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeConsensusProviderFactory;

impl ConsensusNativeProviderFactory for NativeConsensusProviderFactory {
    type Provider = NativeConsensusProvider;

    fn provider(&self) -> Self::Provider {
        NativeConsensusProvider::new()
    }
}
