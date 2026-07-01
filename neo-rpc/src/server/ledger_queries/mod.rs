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

use neo_error::{CoreError, CoreResult};
use neo_native_contracts::LedgerContract;
use neo_payloads::Block;
use neo_primitives::UInt256;
use neo_storage::persistence::DataCache;

use crate::server::model::block_hash_or_index::BlockHashOrIndex;

/// Resolves a block identifier (height or hash) to the block hash, or
/// `None` when an index has no persisted hash entry.
pub(crate) fn resolve_block_hash(
    snapshot: &DataCache,
    identifier: &BlockHashOrIndex,
) -> CoreResult<Option<UInt256>> {
    match identifier {
        BlockHashOrIndex::Index(index) => LedgerContract::new().get_block_hash(snapshot, *index),
        BlockHashOrIndex::Hash(hash) => Ok(Some(*hash)),
    }
}

/// Loads the full block for `identifier`, reconstructing the
/// transaction list from the per-transaction ledger records (C#
/// `LedgerContract.GetBlock`). Returns `Ok(None)` when the block is not
/// persisted.
pub(crate) fn get_full_block(
    snapshot: &DataCache,
    identifier: &BlockHashOrIndex,
) -> CoreResult<Option<Block>> {
    let Some(hash) = resolve_block_hash(snapshot, identifier)? else {
        return Ok(None);
    };
    let ledger = LedgerContract::new();
    let Some(trimmed) = ledger.get_trimmed_block(snapshot, &hash)? else {
        return Ok(None);
    };

    let mut transactions = Vec::with_capacity(trimmed.hashes.len());
    for tx_hash in &trimmed.hashes {
        let transaction = ledger
            .get_transaction_state(snapshot, tx_hash)?
            .and_then(|state| state.transaction)
            .ok_or_else(|| {
                CoreError::invalid_data(format!(
                    "block {hash} references transaction {tx_hash} with no ledger record"
                ))
            })?;
        transactions.push(transaction);
    }

    Ok(Some(Block {
        header: trimmed.header,
        transactions,
    }))
}
