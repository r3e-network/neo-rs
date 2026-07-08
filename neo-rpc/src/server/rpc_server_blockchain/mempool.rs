//! Mempool RPC handlers.
//!
//! This module keeps mempool-specific reads out of the blockchain routing map.
//! Request decoding stays in `request_helpers` and response projection stays in
//! `responses`, so the handler body can focus on the live pool snapshot and
//! ledger height.

use crate::server::rpc_exception::RpcException;
use crate::server::rpc_server::RpcServer;
use serde_json::Value;

use super::RpcServerBlockchain;
use super::ledger_provider::{
    BlockchainLedgerProvider, BlockchainLedgerProviderFactory,
    NativeBlockchainLedgerProviderFactory,
};
use super::request_helpers::RawMemPoolRequest;
use super::responses::{raw_mempool_hashes_to_json, raw_mempool_with_unverified_to_json};

impl RpcServerBlockchain {
    pub(super) fn get_raw_mem_pool(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        if let Some(remote) = server.remote_ledger_rpc() {
            return remote
                .call("getrawmempool", params)
                .map_err(RpcException::from);
        }
        let request = RawMemPoolRequest::parse(params)?;

        let pool = server.system().mempool();
        if !request.include_unverified {
            return Ok(raw_mempool_hashes_to_json(&pool.verified_snapshot()));
        }

        let (verified, unverified) = (pool.verified_snapshot(), pool.unverified_snapshot());

        let store = server.system().store_cache();
        let height = NativeBlockchainLedgerProviderFactory
            .provider()
            .current_height(store.data_cache())?;
        Ok(raw_mempool_with_unverified_to_json(
            height,
            &verified,
            &unverified,
        ))
    }
}
