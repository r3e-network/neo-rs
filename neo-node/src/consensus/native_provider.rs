//! Native-contract read capabilities for consensus orchestration.
//!
//! Consensus needs a narrow Ledger/NEO/Policy view to configure dBFT
//! validators, `NextConsensus`, block timing, tip context, and
//! traceable-conflict windows. Keeping those reads behind a local provider seam
//! makes the node driver depend on capabilities instead of constructing native
//! contracts or ledger providers directly in the consensus flow.

use neo_blockchain::{
    BlockProvider, ChainTipProvider, EmptyLedgerProvider, HotColdLedgerProviderFactory,
    LedgerProviderFactory, TransactionStateProvider, TxProvider,
};
use neo_config::ProtocolSettings;
use neo_crypto::ECPoint;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_primitives::{UInt160, UInt256};
use neo_storage::persistence::{CacheRead, DataCache};
use std::sync::Arc;

const CONSENSUS_LEDGER_PROVIDER_FACTORY: HotColdLedgerProviderFactory<EmptyLedgerProvider> =
    HotColdLedgerProviderFactory::new(EmptyLedgerProvider);

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
    fn ledger_tip<B: CacheRead>(&self, snapshot: &DataCache<B>) -> ConsensusLedgerTip;

    /// Returns the validators for the next block, in C# consensus order.
    fn next_block_validators<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> anyhow::Result<Vec<ECPoint>>;

    /// Returns the `NextConsensus` account for `block_index`.
    fn next_consensus_address_for_block<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
        block_index: u32,
    ) -> anyhow::Result<UInt160>;

    /// Returns the live milliseconds-per-block policy for the round.
    fn milliseconds_per_block<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> anyhow::Result<u32>;

    /// Returns the active `MaxTraceableBlocks` value.
    fn max_traceable_blocks<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> anyhow::Result<u32>;

    /// Returns whether the transaction hash is already persisted on-chain.
    fn contains_transaction<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        hash: &UInt256,
    ) -> anyhow::Result<bool>;

    /// Returns whether the transaction hash has a traceable on-chain conflict
    /// for one of the supplied signers.
    fn contains_conflict_hash<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        hash: &UInt256,
        signers: &[UInt160],
        max_traceable_blocks: u32,
    ) -> anyhow::Result<bool>;
}

/// Adapter from the node-composed native-contract provider to the consensus
/// orchestration read capability.
#[derive(Clone)]
pub(super) struct NativeConsensusProvider<P>
where
    P: NativeContractProvider,
{
    native_contract_provider: Arc<P>,
}

impl<P> NativeConsensusProvider<P>
where
    P: NativeContractProvider,
{
    /// Creates an adapter over the composition-root native-contract provider.
    #[must_use]
    pub(super) fn new(native_contract_provider: Arc<P>) -> Self {
        Self {
            native_contract_provider,
        }
    }

    fn provider(&self) -> &P {
        self.native_contract_provider.as_ref()
    }
}

impl<P> std::fmt::Debug for NativeConsensusProvider<P>
where
    P: NativeContractProvider,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeConsensusProvider")
            .field("native_contract_provider", &"NativeContractProvider")
            .finish()
    }
}

impl<P> ConsensusNativeProvider for NativeConsensusProvider<P>
where
    P: NativeContractProvider,
{
    fn ledger_tip<B: CacheRead>(&self, snapshot: &DataCache<B>) -> ConsensusLedgerTip {
        let ledger = CONSENSUS_LEDGER_PROVIDER_FACTORY.provider(snapshot);
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

    fn next_block_validators<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> anyhow::Result<Vec<ECPoint>> {
        Ok(self.provider().next_block_validators(snapshot, settings)?)
    }

    fn next_consensus_address_for_block<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
        block_index: u32,
    ) -> anyhow::Result<UInt160> {
        Ok(self
            .provider()
            .next_consensus_address_for_block(snapshot, settings, block_index)?)
    }

    fn milliseconds_per_block<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> anyhow::Result<u32> {
        Ok(self.provider().milliseconds_per_block(snapshot, settings)?)
    }

    fn max_traceable_blocks<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> anyhow::Result<u32> {
        Ok(self.provider().max_traceable_blocks(snapshot, settings)?)
    }

    fn contains_transaction<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        hash: &UInt256,
    ) -> anyhow::Result<bool> {
        let ledger = CONSENSUS_LEDGER_PROVIDER_FACTORY.provider(snapshot);
        Ok(ledger.contains_transaction(hash)?)
    }

    fn contains_conflict_hash<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        hash: &UInt256,
        signers: &[UInt160],
        max_traceable_blocks: u32,
    ) -> anyhow::Result<bool> {
        let ledger = CONSENSUS_LEDGER_PROVIDER_FACTORY.provider(snapshot);
        Ok(ledger.contains_conflict_hash(hash, signers, max_traceable_blocks)?)
    }
}
