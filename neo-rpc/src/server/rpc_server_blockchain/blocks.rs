//! Block, header, height, and chain-tip RPC handlers.

use neo_native_contracts::LedgerContract;
use serde_json::{Value, json};

use super::RpcServerBlockchain;
use super::request_helpers::{BlockHeightRequest, BlockPayloadRequest};
use super::responses::{block_to_json, header_to_json};
use crate::server::ledger_queries;
use crate::server::model::block_hash_or_index::BlockHashOrIndex as RpcBlockHashOrIndex;
use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::{internal_error, serialize_to_base64};
use crate::server::rpc_server::RpcServer;

impl RpcServerBlockchain {
    pub(super) fn get_best_block_hash(
        server: &RpcServer,
        _params: &[Value],
    ) -> Result<Value, RpcException> {
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

    pub(super) fn get_block_count(
        server: &RpcServer,
        _params: &[Value],
    ) -> Result<Value, RpcException> {
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

    pub(super) fn get_block_header_count(
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

    pub(super) fn get_block_hash(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        if let Some(remote) = server.remote_ledger_rpc() {
            return remote
                .call("getblockhash", params)
                .map_err(RpcException::from);
        }
        let request = BlockHeightRequest::parse(params, "getblockhash")?;
        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let current = ledger
            .current_index(store.data_cache())
            .map_err(internal_error)?;
        if request.height > current {
            return Err(RpcException::from(RpcError::unknown_height()));
        }

        let hash = ledger
            .get_block_hash(store.data_cache(), request.height)
            .map_err(internal_error)?
            .ok_or_else(|| RpcException::from(RpcError::unknown_block()))?;
        Ok(Value::String(hash.to_string()))
    }

    pub(super) fn get_block(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        if let Some(remote) = server.remote_ledger_rpc() {
            return remote.call("getblock", params).map_err(RpcException::from);
        }
        let request = BlockPayloadRequest::parse(params, "getblock")?;
        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let block = Self::fetch_payload_block(&store, &request.identifier)?;
        if request.verbose {
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

    pub(super) fn get_block_header(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        if let Some(remote) = server.remote_ledger_rpc() {
            return remote
                .call("getblockheader", params)
                .map_err(RpcException::from);
        }
        let request = BlockPayloadRequest::parse(params, "getblockheader")?;
        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let block = Self::fetch_payload_block(&store, &request.identifier)?;
        let header = &block.header;
        if request.verbose {
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

    pub(super) fn get_block_sys_fee(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        if let Some(remote) = server.remote_ledger_rpc() {
            return remote
                .call("getblocksysfee", params)
                .map_err(RpcException::from);
        }
        let request = BlockHeightRequest::parse(params, "getblocksysfee")?;
        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let current = ledger
            .current_index(store.data_cache())
            .map_err(internal_error)?;
        if request.height > current {
            return Err(RpcException::from(RpcError::unknown_height()));
        }

        let block = ledger_queries::get_full_block(
            store.data_cache(),
            &RpcBlockHashOrIndex::Index(request.height),
        )
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
