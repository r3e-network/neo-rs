//! C#-compatible dummy persisting block construction for stateless invokes.

use neo_blockchain::{
    BlockProvider, ChainTipProvider, LedgerProviderFactory, StorageLedgerProviderFactory,
};
use neo_payloads::witness::Witness;
use neo_payloads::{Block, Header};

use super::native_provider::{
    NativeSessionProviderFactory, SessionNativeProvider, SessionNativeProviderFactory,
};

/// Builds the dummy persisting block for a stateless RPC invoke, mirroring C#
/// `ApplicationEngine.CreateDummyBlock(IReadOnlyStore snapshot, ProtocolSettings
/// settings)`.
///
/// The block reads the current (last-persisted) block from the ledger and sets:
/// - `Version = 0`
/// - `PrevHash = LedgerContract.CurrentHash(snapshot)`
/// - `MerkleRoot = UInt256::default()` (C# `new UInt256()`)
/// - `Timestamp = currentBlock.Timestamp + GetTimePerBlock(snapshot, settings)`
///   where the per-block time is the Policy-aware `MillisecondsPerBlock`
///   (static setting pre-HF_Echidna, Policy storage value from HF_Echidna on)
/// - `Index = currentBlock.Index + 1`
/// - `NextConsensus = currentBlock.NextConsensus`
/// - `Witness = Witness.Empty`, `Transactions = []`
/// - `Nonce`/`PrimaryIndex` left at their zero defaults (C# does not set them).
///
/// Returns `None` (leaving the engine without a persisting block, as before)
/// when the ledger has no current block yet, for example a store without a
/// persisted genesis, matching the C# pre-genesis `KeyNotFoundException` corner
/// where a dummy block cannot be constructed.
pub(super) fn create_dummy_block(
    snapshot: &neo_storage::persistence::DataCache,
    settings: &neo_config::ProtocolSettings,
) -> Option<Block> {
    let provider = StorageLedgerProviderFactory.provider(snapshot);
    let current_hash = provider.current_hash().ok()?;
    let current_header = provider.header_by_hash(&current_hash).ok()??;

    let milliseconds_per_block = NativeSessionProviderFactory
        .provider()
        .milliseconds_per_block(snapshot, settings)
        .unwrap_or(settings.milliseconds_per_block);

    let mut header = Header::new();
    header.set_version(0);
    header.set_prev_hash(current_hash);
    header.set_merkle_root(neo_primitives::UInt256::default());
    header.set_timestamp(
        current_header
            .timestamp()
            .saturating_add(u64::from(milliseconds_per_block)),
    );
    header.set_index(current_header.index().saturating_add(1));
    header.set_next_consensus(*current_header.next_consensus());
    header.witness = Witness::empty();

    Some(Block::from_parts(header, Vec::new()))
}
