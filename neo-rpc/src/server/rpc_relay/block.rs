//! Block relay preflight and import through the blockchain service boundary.

use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;
use crate::server::rpc_relay::runtime::block_on_service;
use crate::server::rpc_server::RpcServer;
use neo_blockchain::RelayResult;
use neo_payloads::{Block, InventoryType, VerifyResult};
use neo_runtime::{BlockImport, BlockImportOutcome, BlockOrigin};

/// Relays a block through the blockchain service's import path and
/// returns the outcome as a [`RelayResult`].
pub(in crate::server) fn relay_block(
    server: &RpcServer,
    block: Block,
) -> Result<RelayResult, RpcException> {
    let hash = block
        .header
        .clone()
        .try_hash()
        .map_err(|err| internal_error(err.to_string()))?;
    let index = block.header.index();
    let system = server.system();

    // C# `Blockchain.OnNewBlock` height pre-classification (v3.10.1):
    // a block at or below the persisted height already exists, and a
    // block more than one past the best known header cannot be
    // verified yet. (C# additionally stashes the too-far-ahead block
    // in an actor-internal unverified-block cache for later sync; that
    // cache has no RPC-visible effect, so the adapter only reports the
    // verdict.)
    let store = system.store_cache();
    let current_height = neo_native_contracts::LedgerContract::new()
        .current_index(store.data_cache())
        .map_err(|err| internal_error(err.to_string()))?;
    let header_height = system
        .header_cache()
        .last()
        .map(|header| header.index())
        .unwrap_or(current_height);
    if index <= current_height {
        return Ok(RelayResult {
            hash,
            inventory_type: InventoryType::Block,
            block_index: Some(index),
            result: VerifyResult::AlreadyExists,
        });
    }
    if index.saturating_sub(1) > header_height {
        return Ok(RelayResult {
            hash,
            inventory_type: InventoryType::Block,
            block_index: Some(index),
            result: VerifyResult::UnableToVerify,
        });
    }

    let blockchain = system.blockchain();
    if block_on_service(blockchain.check(&block))?.is_err() {
        return Ok(RelayResult {
            hash,
            inventory_type: InventoryType::Block,
            block_index: Some(index),
            result: VerifyResult::Invalid,
        });
    }
    let outcome = block_on_service(blockchain.import(block, BlockOrigin::Rpc))?
        .map_err(|err| internal_error(err.to_string()))?;
    Ok(RelayResult {
        hash,
        inventory_type: InventoryType::Block,
        block_index: Some(index),
        result: match outcome {
            BlockImportOutcome::Imported(_) => VerifyResult::Succeed,
            // The import path rejected a height-plausible block; the C#
            // `OnNewBlock` verification branches return `Invalid` for these.
            BlockImportOutcome::NotImported { .. } => VerifyResult::Invalid,
        },
    })
}

#[cfg(test)]
#[path = "../../tests/server/rpc_relay/block.rs"]
mod tests;
