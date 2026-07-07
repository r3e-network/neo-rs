//! # neo-rpc::server::rpc_relay
//!
//! Relay helpers that submit transactions through the node boundary.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `result`: C#-compatible relay-result projection into JSON-RPC responses.
//! - `runtime`: synchronous bridge for async service calls.

mod result;
mod runtime;

use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;
use crate::server::rpc_server::RpcServer;
use neo_blockchain::RelayResult;
use neo_payloads::VerifyResult;
use neo_payloads::{Block, InventoryType, Transaction};
use neo_runtime::{BlockImport, BlockImportOutcome, BlockOrigin};

pub(super) use result::map_relay_result;
use runtime::block_on_service;

/// Relays a transaction through the blockchain service's mempool
/// admission path and returns the verify outcome as a [`RelayResult`].
///
/// The legacy actor flow ferried a `RelayResult` message back through a
/// responder actor; in the reth-style service world the same outcome is
/// the `AddTransactionReply` of [`neo_blockchain::BlockchainHandle::add_transaction`].
pub(super) fn relay_transaction(
    server: &RpcServer,
    transaction: Transaction,
) -> Result<RelayResult, RpcException> {
    let blockchain = server.system().blockchain();
    let reply = block_on_service(blockchain.add_transaction(transaction))?
        .map_err(|err| internal_error(err.to_string()))?;
    Ok(RelayResult {
        hash: reply.hash,
        inventory_type: InventoryType::Transaction,
        block_index: None,
        result: reply.result,
    })
}

/// Relays a block through the blockchain service's import path and
/// returns the outcome as a [`RelayResult`].
pub(super) fn relay_block(server: &RpcServer, block: Block) -> Result<RelayResult, RpcException> {
    let hash = block
        .header
        .clone()
        .try_hash()
        .map_err(|err| internal_error(err.to_string()))?;
    let index = block.header.index();
    let system = server.system();

    // C# `Blockchain.OnNewBlock` height pre-classification (v3.10.0):
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
            // The import path rejected a height-plausible block — the
            // C# `OnNewBlock` verification branches return `Invalid`
            // for these.
            BlockImportOutcome::NotImported { .. } => VerifyResult::Invalid,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::rpc_server_settings::RpcServerConfig;
    use neo_config::ProtocolSettings;
    use neo_payloads::Header;
    use neo_primitives::UInt256;

    #[tokio::test(flavor = "multi_thread")]
    async fn relay_block_preflight_rejects_bad_merkle_root_as_invalid() {
        let system = crate::server::test_support::test_system(ProtocolSettings::default());
        let server = RpcServer::new(system, RpcServerConfig::default());
        let mut header = Header::new();
        header.set_index(1);
        header.set_merkle_root(UInt256::from([0x42; 32]));
        let block = Block::from_parts(header, Vec::new());

        let result = relay_block(&server, block).expect("relay result");

        assert_eq!(result.inventory_type, InventoryType::Block);
        assert_eq!(result.block_index, Some(1));
        assert_eq!(result.result, VerifyResult::Invalid);
    }
}
