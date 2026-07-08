//! # neo-rpc::server::ledger_queries
//!
//! Shared ledger query helpers used by RPC handlers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `ledger_queries`: ledger query helpers shared by RPC endpoints.

use neo_blockchain::ledger_provider::{
    BlockProvider, ChainTipProvider, LedgerProviderFactory, StorageLedgerProviderFactory,
};
use neo_error::CoreResult;
use neo_payloads::Block;
use neo_primitives::UInt256;
use neo_storage::persistence::DataCache;

use crate::server::model::block_hash_or_index::BlockHashOrIndex;

/// Ledger context attached to a verbose transaction response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TransactionLedgerContext {
    /// Number of confirmations relative to the current persisted height.
    pub(crate) confirmations: u32,
    /// Block hash for the transaction height, when the ledger index exists.
    pub(crate) block_hash: Option<UInt256>,
    /// Block timestamp for `block_hash`, when the header is still available.
    pub(crate) block_time: Option<u64>,
}

/// Resolves a block identifier (height or hash) to the block hash, or
/// `None` when an index has no persisted hash entry.
///
/// Routes the index case through [`StorageLedgerProviderFactory`], whose
/// provider directly forwards to `LedgerContract::get_block_hash` —
/// behaviourally identical to the previous inline call.
pub(crate) fn resolve_block_hash(
    snapshot: &DataCache,
    identifier: &BlockHashOrIndex,
) -> CoreResult<Option<UInt256>> {
    match identifier {
        BlockHashOrIndex::Index(index) => StorageLedgerProviderFactory
            .provider(snapshot)
            .block_hash_by_index(*index),
        BlockHashOrIndex::Hash(hash) => Ok(Some(*hash)),
    }
}

/// Returns the current persisted ledger height through the canonical storage
/// ledger provider.
///
/// Endpoint-local provider traits should call this helper instead of
/// constructing [`StorageLedgerProviderFactory`] directly. That keeps each RPC
/// handler on a narrow capability trait while keeping the raw storage-provider
/// boundary in one shared module.
pub(crate) fn current_index(snapshot: &DataCache) -> CoreResult<u32> {
    StorageLedgerProviderFactory
        .provider(snapshot)
        .current_index()
}

/// Returns the current persisted ledger hash through the canonical storage
/// ledger provider.
pub(crate) fn current_hash(snapshot: &DataCache) -> CoreResult<UInt256> {
    StorageLedgerProviderFactory
        .provider(snapshot)
        .current_hash()
}

/// Returns the current persisted block count through the canonical storage
/// ledger provider.
pub(crate) fn block_count(snapshot: &DataCache) -> CoreResult<u32> {
    StorageLedgerProviderFactory
        .provider(snapshot)
        .block_count()
}

/// Returns the canonical block hash for `index` through the shared storage
/// ledger boundary.
pub(crate) fn block_hash_by_index(snapshot: &DataCache, index: u32) -> CoreResult<Option<UInt256>> {
    StorageLedgerProviderFactory
        .provider(snapshot)
        .block_hash_by_index(index)
}

/// Returns the current height plus the canonical next block hash for the block
/// or header at `index`.
pub(crate) fn current_index_and_next_hash(
    snapshot: &DataCache,
    index: u32,
) -> CoreResult<(u32, Option<UInt256>)> {
    let provider = StorageLedgerProviderFactory.provider(snapshot);
    let current_index = provider.current_index()?;
    let next_hash = provider.block_hash_by_index(index.saturating_add(1))?;
    Ok((current_index, next_hash))
}

/// Returns the ledger metadata that C# adds to verbose transaction JSON:
/// confirmations, block hash, and block timestamp.
pub(crate) fn transaction_context(
    snapshot: &DataCache,
    block_index: u32,
) -> CoreResult<TransactionLedgerContext> {
    let provider = StorageLedgerProviderFactory.provider(snapshot);
    let current_index = provider.current_index()?;
    let confirmations = current_index.saturating_sub(block_index).saturating_add(1);
    let block_hash = provider.block_hash_by_index(block_index)?;
    let block_time = match block_hash {
        Some(hash) => provider
            .header_by_hash(&hash)?
            .map(|header| header.timestamp()),
        None => None,
    };

    Ok(TransactionLedgerContext {
        confirmations,
        block_hash,
        block_time,
    })
}

/// Loads the full block for `identifier`, reconstructing the
/// transaction list from the per-transaction ledger records (C#
/// `LedgerContract.GetBlock`). Returns `Ok(None)` when the block is not
/// persisted.
///
/// The hash resolution and the trimmed-block + per-transaction reconstruction
/// are both delegated through [`StorageLedgerProviderFactory`], the canonical
/// hot ledger read factory. Its provider performs the same reconstruction
/// (same trimmed header, same transaction order, and the same
/// `CoreError::invalid_data` message when a referenced transaction has no
/// ledger record), so the result is byte-identical to the former hand-rolled
/// loop.
pub(crate) fn get_full_block(
    snapshot: &DataCache,
    identifier: &BlockHashOrIndex,
) -> CoreResult<Option<Block>> {
    let Some(hash) = resolve_block_hash(snapshot, identifier)? else {
        return Ok(None);
    };
    StorageLedgerProviderFactory
        .provider(snapshot)
        .block_by_hash(&hash)
}
