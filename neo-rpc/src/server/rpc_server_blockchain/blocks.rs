//! Block, header, height, and chain-tip RPC handlers.

use serde_json::Value;

use super::RpcServerBlockchain;
use super::request_helpers::{BlockHeightRequest, BlockPayloadRequest, NoParamsRequest};
use super::responses::{
    base64_payload_to_json, block_to_json, count_to_json, hash_to_json, header_to_json,
    system_fee_to_json,
};
use crate::server::ledger_queries;
use crate::server::model::block_hash_or_index::BlockHashOrIndex as RpcBlockHashOrIndex;
use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::{internal_error, serialize_to_base64};
use crate::server::rpc_server::RpcServer;

impl RpcServerBlockchain {
    pub(super) fn get_best_block_hash(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        NoParamsRequest::parse(params, "getbestblockhash")?;
        if let Some(remote) = server.remote_ledger_rpc() {
            return remote
                .call("getbestblockhash", &[])
                .map_err(RpcException::from);
        }
        let store = server.system().store_cache();
        let hash = ledger_queries::current_hash(store.data_cache()).map_err(internal_error)?;
        Ok(hash_to_json(&hash))
    }

    pub(super) fn get_block_count(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        NoParamsRequest::parse(params, "getblockcount")?;
        if let Some(remote) = server.remote_ledger_rpc() {
            return remote
                .call("getblockcount", &[])
                .map_err(RpcException::from);
        }
        let store = server.system().store_cache();
        let count = ledger_queries::block_count(store.data_cache()).map_err(internal_error)?;
        Ok(count_to_json(count))
    }

    pub(super) fn get_block_header_count(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        NoParamsRequest::parse(params, "getblockheadercount")?;
        if let Some(remote) = server.remote_ledger_rpc() {
            return remote
                .call("getblockheadercount", &[])
                .map_err(RpcException::from);
        }
        let system = server.system();
        let header_cache = system.header_cache();
        let cache_height = header_cache.last().map(|header| header.index());
        let store = system.store_cache();
        let base_height = if let Some(index) = cache_height {
            index
        } else {
            ledger_queries::current_index(store.data_cache()).map_err(internal_error)?
        };
        Ok(count_to_json(base_height.saturating_add(1)))
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
        let current = ledger_queries::current_index(store.data_cache()).map_err(internal_error)?;
        if request.height > current {
            return Err(RpcException::from(RpcError::unknown_height()));
        }

        let hash = ledger_queries::block_hash_by_index(store.data_cache(), request.height)
            .map_err(internal_error)?
            .ok_or_else(|| RpcException::from(RpcError::unknown_block()))?;
        Ok(hash_to_json(&hash))
    }

    pub(super) fn get_block(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        if let Some(remote) = server.remote_ledger_rpc() {
            return remote.call("getblock", params).map_err(RpcException::from);
        }
        let request = BlockPayloadRequest::parse(params, "getblock")?;
        let store = server.system().store_cache();
        let block = Self::fetch_payload_block(&store, &request.identifier)?;
        if request.verbose {
            let (current_index, next_hash) = ledger_queries::current_index_and_next_hash(
                store.data_cache(),
                block.header.index(),
            )
            .map_err(internal_error)?;
            return Ok(block_to_json(server, &block, current_index, next_hash));
        }

        Ok(base64_payload_to_json(serialize_to_base64(&block)?))
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
        let block = Self::fetch_payload_block(&store, &request.identifier)?;
        let header = &block.header;
        if request.verbose {
            let (current_index, next_hash) =
                ledger_queries::current_index_and_next_hash(store.data_cache(), header.index())
                    .map_err(internal_error)?;
            return Ok(header_to_json(server, header, current_index, next_hash));
        }

        Ok(base64_payload_to_json(serialize_to_base64(header)?))
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
        let current = ledger_queries::current_index(store.data_cache()).map_err(internal_error)?;
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
        Ok(system_fee_to_json(system_fee))
    }
}
