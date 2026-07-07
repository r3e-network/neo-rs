//! # neo-rpc::server::rpc_server_blockchain
//!
//! Blockchain RPC endpoint handlers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `blocks`: Block, header, height, and chain-tip RPC handlers.
//! - `request_helpers`: RPC request parsing helpers.
//! - `responses`: RPC response construction helpers.
//! - `mempool`: Mempool RPC handlers.
//! - `native`: Native contract and governance RPC handlers.
//! - `storage`: Contract-state and contract-storage RPC handlers.
//! - `transactions`: Transaction RPC handlers.
//! - `tests`: Module-local tests and regression coverage.

use crate::server::rpc_server::RpcHandler;

mod blocks;
mod mempool;
mod native;
mod request_helpers;
mod responses;
mod storage;
mod transactions;

/// RPC handler group for blockchain query methods.
pub struct RpcServerBlockchain;

impl RpcServerBlockchain {
    /// Register blockchain RPC handlers.
    pub fn register_handlers() -> Vec<RpcHandler> {
        super::rpc_handlers![
            "getbestblockhash" => Self::get_best_block_hash,
            "getblockcount" => Self::get_block_count,
            "getblockheadercount" => Self::get_block_header_count,
            "getblockhash" => Self::get_block_hash,
            "getblock" => Self::get_block,
            "getblockheader" => Self::get_block_header,
            "getblocksysfee" => Self::get_block_sys_fee,
            "getrawmempool" => Self::get_raw_mem_pool,
            "getrawtransaction" => Self::get_raw_transaction,
            "getcontractstate" => Self::get_contract_state,
            "getstorage" => Self::get_storage,
            "findstorage" => Self::find_storage,
            "getnativecontracts" => Self::get_native_contracts,
            "getnextblockvalidators" => Self::get_next_block_validators,
            "getcandidates" => Self::get_candidates,
            "gettransactionheight" => Self::get_transaction_height,
            "getcommittee" => Self::get_committee,
        ]
    }
}

#[cfg(test)]
#[path = "../../tests/server/handlers/rpc_server_blockchain.rs"]
mod tests;
