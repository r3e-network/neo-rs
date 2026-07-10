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
    BlockProvider, ChainTipProvider, EmptyLedgerProvider, HotColdLedgerProviderFactory,
    LedgerProviderFactory,
};
use neo_error::CoreResult;
use neo_payloads::Block;
use neo_primitives::UInt256;
use neo_storage::persistence::{CacheRead, DataCache};

use crate::server::model::block_hash_or_index::BlockHashOrIndex;

const LEDGER_QUERY_PROVIDER_FACTORY: HotColdLedgerProviderFactory<EmptyLedgerProvider> =
    HotColdLedgerProviderFactory::new(EmptyLedgerProvider);

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
/// Routes the index case through the shared hot/cold ledger provider factory.
/// With [`EmptyLedgerProvider`] as the cold side, behavior matches hot native
/// Ledger storage today while keeping the static-file archive seam explicit.
pub(crate) fn resolve_block_hash<B: CacheRead>(
    snapshot: &DataCache<B>,
    identifier: &BlockHashOrIndex,
) -> CoreResult<Option<UInt256>> {
    match identifier {
        BlockHashOrIndex::Index(index) => LEDGER_QUERY_PROVIDER_FACTORY
            .provider(snapshot)
            .block_hash_by_index(*index),
        BlockHashOrIndex::Hash(hash) => Ok(Some(*hash)),
    }
}

/// Returns the current persisted ledger height through the routed
/// ledger provider.
///
/// Endpoint-local provider traits should call this helper instead of
/// constructing ledger providers directly. That keeps each RPC handler on a
/// narrow capability trait while keeping the hot/cold routing boundary in one
/// shared module.
pub(crate) fn current_index<B: CacheRead>(snapshot: &DataCache<B>) -> CoreResult<u32> {
    LEDGER_QUERY_PROVIDER_FACTORY
        .provider(snapshot)
        .current_index()
}

/// Returns the current persisted ledger hash through the routed
/// ledger provider.
pub(crate) fn current_hash<B: CacheRead>(snapshot: &DataCache<B>) -> CoreResult<UInt256> {
    LEDGER_QUERY_PROVIDER_FACTORY
        .provider(snapshot)
        .current_hash()
}

/// Returns the current persisted block count through the routed
/// ledger provider.
pub(crate) fn block_count<B: CacheRead>(snapshot: &DataCache<B>) -> CoreResult<u32> {
    LEDGER_QUERY_PROVIDER_FACTORY
        .provider(snapshot)
        .block_count()
}

/// Returns the canonical block hash for `index` through the shared storage
/// ledger boundary.
pub(crate) fn block_hash_by_index<B: CacheRead>(
    snapshot: &DataCache<B>,
    index: u32,
) -> CoreResult<Option<UInt256>> {
    LEDGER_QUERY_PROVIDER_FACTORY
        .provider(snapshot)
        .block_hash_by_index(index)
}

/// Returns the current height plus the canonical next block hash for the block
/// or header at `index`.
pub(crate) fn current_index_and_next_hash<B: CacheRead>(
    snapshot: &DataCache<B>,
    index: u32,
) -> CoreResult<(u32, Option<UInt256>)> {
    let provider = LEDGER_QUERY_PROVIDER_FACTORY.provider(snapshot);
    let current_index = provider.current_index()?;
    let next_hash = provider.block_hash_by_index(index.saturating_add(1))?;
    Ok((current_index, next_hash))
}

/// Returns the ledger metadata that C# adds to verbose transaction JSON:
/// confirmations, block hash, and block timestamp.
pub(crate) fn transaction_context<B: CacheRead>(
    snapshot: &DataCache<B>,
    block_index: u32,
) -> CoreResult<TransactionLedgerContext> {
    let provider = LEDGER_QUERY_PROVIDER_FACTORY.provider(snapshot);
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
/// are both delegated through the routed hot/cold ledger read factory. Its
/// current empty-cold configuration performs the same hot reconstruction (same
/// trimmed header, same transaction order, and the same
/// `CoreError::invalid_data` message when a referenced transaction has no
/// ledger record), so the result is byte-identical to the former hand-rolled
/// loop.
pub(crate) fn get_full_block<B: CacheRead>(
    snapshot: &DataCache<B>,
    identifier: &BlockHashOrIndex,
) -> CoreResult<Option<Block>> {
    let Some(hash) = resolve_block_hash(snapshot, identifier)? else {
        return Ok(None);
    };
    LEDGER_QUERY_PROVIDER_FACTORY
        .provider(snapshot)
        .block_by_hash(&hash)
}
