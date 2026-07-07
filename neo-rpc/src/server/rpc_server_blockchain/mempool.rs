//! Mempool RPC handlers.
//!
//! This module keeps mempool-specific reads and response construction out of
//! the blockchain routing map. Request decoding stays in `request_helpers` so
//! the handler body can focus on the live pool snapshot and ledger height.

use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;
use crate::server::rpc_server::RpcServer;
use neo_native_contracts::LedgerContract;
use serde_json::{Value, json};

use super::RpcServerBlockchain;
use super::request_helpers::RawMemPoolRequest;

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
            let hashes: Vec<Value> = pool
                .verified_snapshot()
                .iter()
                .map(|item| Value::String(item.hash().to_string()))
                .collect();
            return Ok(Value::Array(hashes));
        }

        let (verified, unverified) = (pool.verified_snapshot(), pool.unverified_snapshot());

        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let height = ledger
            .current_index(store.data_cache())
            .map_err(internal_error)?;
        let verified_hashes: Vec<Value> = verified
            .iter()
            .map(|item| Value::String(item.hash().to_string()))
            .collect();
        let unverified_hashes: Vec<Value> = unverified
            .iter()
            .map(|item| Value::String(item.hash().to_string()))
            .collect();

        Ok(json!({
            "height": height,
            "verified": verified_hashes,
            "unverified": unverified_hashes}))
    }
}
