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
use neo_execution::NativeContract;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_native_contracts::{NeoToken, PolicyContract};
use neo_primitives::{UInt160, UInt256};
use neo_storage::persistence::DataCache;
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

/// Adapter from the node-composed native-contract provider to the consensus
/// orchestration read capability.
#[derive(Clone)]
pub(super) struct NativeConsensusProvider {
    native_contract_provider: Arc<dyn NativeContractProvider>,
}

impl NativeConsensusProvider {
    /// Creates an adapter over the composition-root native-contract provider.
    #[must_use]
    pub(super) fn new(native_contract_provider: Arc<dyn NativeContractProvider>) -> Self {
        Self {
            native_contract_provider,
        }
    }

    fn provider(&self) -> Arc<dyn NativeContractProvider> {
        Arc::clone(&self.native_contract_provider)
    }

    fn neo_token(&self) -> anyhow::Result<Arc<dyn NativeContract>> {
        self.provider()
            .get_native_contract_by_name("NeoToken")
            .ok_or_else(|| anyhow::anyhow!("native provider missing NeoToken"))
    }

    fn policy_contract(&self) -> anyhow::Result<Arc<dyn NativeContract>> {
        self.provider()
            .get_native_contract_by_name("PolicyContract")
            .ok_or_else(|| anyhow::anyhow!("native provider missing PolicyContract"))
    }
}

impl std::fmt::Debug for NativeConsensusProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeConsensusProvider")
            .field("native_contract_provider", &"NativeContractProvider")
            .finish()
    }
}

impl ConsensusNativeProvider for NativeConsensusProvider {
    fn ledger_tip(&self, snapshot: &DataCache) -> ConsensusLedgerTip {
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

    fn next_block_validators(
        &self,
        snapshot: &DataCache,
        validators_count: usize,
    ) -> anyhow::Result<Vec<ECPoint>> {
        Ok(self
            .neo_token()?
            .as_any()
            .downcast_ref::<NeoToken>()
            .ok_or_else(|| anyhow::anyhow!("native provider returned non-NeoToken"))?
            .next_block_validators(snapshot, validators_count)?)
    }

    fn next_consensus_address_for_block(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
        block_index: u32,
    ) -> anyhow::Result<UInt160> {
        Ok(self
            .neo_token()?
            .as_any()
            .downcast_ref::<NeoToken>()
            .ok_or_else(|| anyhow::anyhow!("native provider returned non-NeoToken"))?
            .next_consensus_address_for_block(snapshot, settings, block_index)?)
    }

    fn milliseconds_per_block(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> anyhow::Result<u32> {
        Ok(self
            .policy_contract()?
            .as_any()
            .downcast_ref::<PolicyContract>()
            .ok_or_else(|| anyhow::anyhow!("native provider returned non-PolicyContract"))?
            .get_milliseconds_per_block_snapshot(snapshot, settings)?)
    }

    fn max_traceable_blocks(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> anyhow::Result<u32> {
        Ok(self
            .policy_contract()?
            .as_any()
            .downcast_ref::<PolicyContract>()
            .ok_or_else(|| anyhow::anyhow!("native provider returned non-PolicyContract"))?
            .get_max_traceable_blocks_snapshot(snapshot, settings)?)
    }

    fn contains_transaction(&self, snapshot: &DataCache, hash: &UInt256) -> anyhow::Result<bool> {
        let ledger = CONSENSUS_LEDGER_PROVIDER_FACTORY.provider(snapshot);
        Ok(ledger.contains_transaction(hash)?)
    }

    fn contains_conflict_hash(
        &self,
        snapshot: &DataCache,
        hash: &UInt256,
        signers: &[UInt160],
        max_traceable_blocks: u32,
    ) -> anyhow::Result<bool> {
        let ledger = CONSENSUS_LEDGER_PROVIDER_FACTORY.provider(snapshot);
        Ok(ledger.contains_conflict_hash(hash, signers, max_traceable_blocks)?)
    }
}
