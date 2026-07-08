//! Native-contract read capabilities for consensus orchestration.
//!
//! Consensus needs a narrow Ledger/NEO/Policy view to configure dBFT
//! validators, `NextConsensus`, block timing, tip context, and
//! traceable-conflict windows. Keeping those reads behind a local provider seam
//! makes the node driver depend on capabilities instead of constructing native
//! contracts or ledger providers directly in the consensus flow.

use neo_blockchain::{
    BlockProvider, ChainTipProvider, LedgerProviderFactory, StorageLedgerProviderFactory,
    TransactionStateProvider, TxProvider,
};
use neo_config::ProtocolSettings;
use neo_crypto::ECPoint;
use neo_native_contracts::{NeoToken, PolicyContract};
use neo_primitives::{UInt160, UInt256};
use neo_storage::persistence::DataCache;

/// Current persisted ledger context used to start a dBFT round.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ConsensusLedgerTip {
    /// Block index the next consensus round should propose.
    pub(super) next_block_index: u32,
    /// Hash of the current persisted tip.
    pub(super) prev_hash: UInt256,
    /// Timestamp of the current persisted tip header, in milliseconds.
    pub(super) prev_timestamp: u64,
}

/// Native-contract capabilities required by consensus orchestration.
pub(super) trait ConsensusNativeProvider {
    /// Returns the current ledger tip context for the next consensus round.
    fn ledger_tip(&self, snapshot: &DataCache) -> ConsensusLedgerTip;

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

    /// Returns whether the transaction hash is already persisted on-chain.
    fn contains_transaction(&self, snapshot: &DataCache, hash: &UInt256) -> anyhow::Result<bool>;

    /// Returns whether the transaction hash has a traceable on-chain conflict
    /// for one of the supplied signers.
    fn contains_conflict_hash(
        &self,
        snapshot: &DataCache,
        hash: &UInt256,
        signers: &[UInt160],
        max_traceable_blocks: u32,
    ) -> anyhow::Result<bool>;
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
    fn ledger_tip(&self, snapshot: &DataCache) -> ConsensusLedgerTip {
        let ledger = StorageLedgerProviderFactory.provider(snapshot);
        let height = ledger.current_index().unwrap_or(0);
        let prev_hash = ledger.current_hash().unwrap_or_default();
        let prev_timestamp = ledger
            .header_by_hash(&prev_hash)
            .ok()
            .flatten()
            .map(|header| header.timestamp())
            .unwrap_or(0);
        ConsensusLedgerTip {
            next_block_index: height + 1,
            prev_hash,
            prev_timestamp,
        }
    }

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

    fn contains_transaction(&self, snapshot: &DataCache, hash: &UInt256) -> anyhow::Result<bool> {
        let ledger = StorageLedgerProviderFactory.provider(snapshot);
        Ok(ledger.contains_transaction(hash)?)
    }

    fn contains_conflict_hash(
        &self,
        snapshot: &DataCache,
        hash: &UInt256,
        signers: &[UInt160],
        max_traceable_blocks: u32,
    ) -> anyhow::Result<bool> {
        let ledger = StorageLedgerProviderFactory.provider(snapshot);
        Ok(ledger.contains_conflict_hash(hash, signers, max_traceable_blocks)?)
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
