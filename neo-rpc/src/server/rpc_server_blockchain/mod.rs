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
//! - `request_helpers`: RPC request parsing helpers.
//! - `responses`: RPC response construction helpers.
//! - `mempool`: Mempool RPC handlers.
//! - `native`: Native contract and governance RPC handlers.
//! - `storage`: Contract-state and contract-storage RPC handlers.
//! - `transactions`: Transaction RPC handlers.
//! - `tests`: Module-local tests and regression coverage.

use crate::server::model::block_hash_or_index::BlockHashOrIndex as RpcBlockHashOrIndex;
use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::{internal_error, serialize_to_base64};
use crate::server::rpc_server::{RpcHandler, RpcServer};
use neo_native_contracts::LedgerContract;

use crate::server::ledger_queries;
use serde_json::{Value, json};

mod mempool;
mod native;
mod request_helpers;
mod responses;
mod storage;
mod transactions;
use responses::{block_to_json, header_to_json};

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

    fn get_best_block_hash(server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
        if let Some(remote) = server.remote_ledger_rpc() {
            return remote
                .call("getbestblockhash", &[])
                .map_err(RpcException::from);
        }
        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let hash = ledger
            .current_hash(store.data_cache())
            .map_err(internal_error)?;
        Ok(Value::String(hash.to_string()))
    }

    fn get_block_count(server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
        if let Some(remote) = server.remote_ledger_rpc() {
            return remote
                .call("getblockcount", &[])
                .map_err(RpcException::from);
        }
        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let count = ledger
            .current_index(store.data_cache())
            .map_err(internal_error)?
            .saturating_add(1);
        Ok(json!(count))
    }

    fn get_block_header_count(
        server: &RpcServer,
        _params: &[Value],
    ) -> Result<Value, RpcException> {
        if let Some(remote) = server.remote_ledger_rpc() {
            return remote
                .call("getblockheadercount", &[])
                .map_err(RpcException::from);
        }
        let system = server.system();
        let header_cache = system.header_cache();
        let cache_height = header_cache.last().map(|header| header.index());
        let store = system.store_cache();
        let ledger = LedgerContract::new();
        let base_height = if let Some(index) = cache_height {
            index
        } else {
            ledger
                .current_index(store.data_cache())
                .map_err(internal_error)?
        };
        Ok(json!(base_height.saturating_add(1)))
    }

    fn get_block_hash(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        if let Some(remote) = server.remote_ledger_rpc() {
            return remote
                .call("getblockhash", params)
                .map_err(RpcException::from);
        }
        let height = Self::expect_u32_param(params, 0, "getblockhash")?;
        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let current = ledger
            .current_index(store.data_cache())
            .map_err(internal_error)?;
        if height > current {
            return Err(RpcException::from(RpcError::unknown_height()));
        }

        let hash = ledger
            .get_block_hash(store.data_cache(), height)
            .map_err(internal_error)?
            .ok_or_else(|| RpcException::from(RpcError::unknown_block()))?;
        Ok(Value::String(hash.to_string()))
    }

    fn get_block(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        if let Some(remote) = server.remote_ledger_rpc() {
            return remote.call("getblock", params).map_err(RpcException::from);
        }
        let identifier = Self::parse_block_identifier(params, "getblock")?;
        let verbose = Self::parse_verbose(params.get(1))?;
        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let block = Self::fetch_payload_block(&store, &identifier)?;
        if verbose {
            let current_index = ledger
                .current_index(store.data_cache())
                .map_err(internal_error)?;
            let next_hash = ledger
                .get_block_hash(store.data_cache(), block.header.index().saturating_add(1))
                .map_err(internal_error)?;
            return Ok(block_to_json(server, &block, current_index, next_hash));
        }

        Ok(Value::String(serialize_to_base64(&block)?))
    }

    fn get_block_header(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        if let Some(remote) = server.remote_ledger_rpc() {
            return remote
                .call("getblockheader", params)
                .map_err(RpcException::from);
        }
        let identifier = Self::parse_block_identifier(params, "getblockheader")?;
        let verbose = Self::parse_verbose(params.get(1))?;
        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let block = Self::fetch_payload_block(&store, &identifier)?;
        let header = &block.header;
        if verbose {
            let current_index = ledger
                .current_index(store.data_cache())
                .map_err(internal_error)?;
            let next_hash = ledger
                .get_block_hash(store.data_cache(), header.index().saturating_add(1))
                .map_err(internal_error)?;
            return Ok(header_to_json(server, header, current_index, next_hash));
        }

        Ok(Value::String(serialize_to_base64(header)?))
    }

    fn get_block_sys_fee(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        if let Some(remote) = server.remote_ledger_rpc() {
            return remote
                .call("getblocksysfee", params)
                .map_err(RpcException::from);
        }
        let height = Self::expect_u32_param(params, 0, "getblocksysfee")?;
        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let current = ledger
            .current_index(store.data_cache())
            .map_err(internal_error)?;
        if height > current {
            return Err(RpcException::from(RpcError::unknown_height()));
        }

        let block =
            ledger_queries::get_full_block(store.data_cache(), &RpcBlockHashOrIndex::Index(height))
                .map_err(internal_error)?
                .ok_or_else(|| RpcException::from(RpcError::unknown_block()))?;

        let system_fee: i64 = block
            .transactions
            .iter()
            .map(neo_payloads::Transaction::system_fee)
            .sum();
        Ok(Value::String(system_fee.to_string()))
    }
}

#[cfg(test)]
#[path = "../../tests/server/handlers/rpc_server_blockchain.rs"]
mod tests;
