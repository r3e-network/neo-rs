//! Transaction relay submission through the blockchain service boundary.

use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;
use crate::server::rpc_relay::runtime::block_on_service;
use crate::server::rpc_server::RpcServer;
use neo_blockchain::RelayResult;
use neo_mempool::TransactionOrigin;
use neo_payloads::{InventoryType, Transaction};

/// Relays a transaction through the blockchain service's mempool
/// admission path and returns the verify outcome as a [`RelayResult`].
///
/// The legacy actor flow ferried a `RelayResult` message back through a
/// responder actor; in the reth-style service world the same outcome is
/// the `AddTransactionReply` of [`neo_blockchain::BlockchainHandle::add_transaction`].
pub(in crate::server) fn relay_transaction(
    server: &RpcServer,
    transaction: Transaction,
) -> Result<RelayResult, RpcException> {
    let blockchain = server.system().blockchain();
    let relay_transaction = transaction.clone();
    let reply =
        block_on_service(blockchain.add_transaction(TransactionOrigin::Local, transaction))?
            .map_err(|err| internal_error(err.to_string()))?;
    if reply.result.is_success() {
        let _ = server
            .system()
            .network()
            .try_broadcast_transaction(relay_transaction);
    }
    Ok(RelayResult {
        hash: reply.hash,
        inventory_type: InventoryType::Transaction,
        block_index: None,
        result: reply.result,
    })
}

#[cfg(test)]
#[path = "../../tests/server/rpc_relay/transaction.rs"]
mod tests;
