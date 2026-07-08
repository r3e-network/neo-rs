//! # neo-rpc::server::rpc_server_indexer
//!
//! Indexer-backed RPC endpoint handlers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `blocks`: Block-index RPC endpoint handlers.
//! - `ledger_provider`: Ledger read seam for indexer endpoint status.
//! - `notifications`: Notification-index RPC endpoint handlers.
//! - `params`: RPC endpoint parameter records.
//! - `responses`: RPC response construction helpers.
//! - `status`: RPC status response records.
//! - `support`: Shared support types and service helpers.
//! - `tests`: Module-local tests and regression coverage.
//! - `transactions`: Transaction-index RPC endpoint handlers.

use crate::server::rpc_server::RpcHandler;

mod blocks;
mod ledger_provider;
mod notifications;
mod params;
mod responses;
mod status;
mod support;
mod transactions;

/// RPC method group for the read-side Neo indexer service.
pub struct RpcServerIndexer;

impl RpcServerIndexer {
    /// Registers NeoIndexer RPC handlers.
    pub fn register_handlers() -> Vec<RpcHandler> {
        super::rpc_handlers![
            "getindexerstatus" => Self::get_indexer_status,
            "getblockindex" => Self::get_block_index,
            "getblockindexes" => Self::get_block_indexes,
            "gettransactionindex" => Self::get_transaction_index,
            "getblocktransactions" => Self::get_block_transactions,
            "getaddresstransactions" => Self::get_address_transactions,
            "getcontracttransactions" => Self::get_contract_transactions,
            "getaddressnotifications" => Self::get_address_notifications,
            "getblocknotifications" => Self::get_block_notifications,
            "gettransactionnotifications" => Self::get_transaction_notifications,
            "getcontractnotifications" => Self::get_contract_notifications,
        ]
    }
}

#[cfg(test)]
#[path = "../../tests/server/handlers/rpc_server_indexer.rs"]
mod tests;
