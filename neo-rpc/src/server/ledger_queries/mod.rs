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
    BlockProvider, LedgerProviderFactory, StorageLedgerProviderFactory,
};
use neo_error::CoreResult;
use neo_payloads::Block;
use neo_primitives::UInt256;
use neo_storage::persistence::DataCache;

use crate::server::model::block_hash_or_index::BlockHashOrIndex;

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
