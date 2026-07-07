//! Remote-ledger JSON-RPC client facade.

use std::thread;

use serde_json::Value;

use super::transport::call_remote_ledger_blocking;
use crate::server::rpc_error::RpcError;

/// Blocking JSON-RPC client used by server handlers when this node delegates
/// ledger reads to an upstream RPC endpoint.
#[derive(Clone)]
pub struct RemoteLedgerRpcClient {
    endpoint: String,
}

impl RemoteLedgerRpcClient {
    /// Create a remote ledger RPC proxy for `endpoint`.
    pub fn new(endpoint: impl Into<String>) -> Result<Self, RpcError> {
        Ok(Self {
            endpoint: endpoint.into(),
        })
    }

    /// Invoke `method` against the upstream RPC endpoint and return its result.
    pub fn call(&self, method: &str, params: &[Value]) -> Result<Value, RpcError> {
        let endpoint = self.endpoint.clone();
        let method = method.to_owned();
        let params = params.to_vec();
        thread::spawn(move || call_remote_ledger_blocking(endpoint, method, params))
            .join()
            .map_err(|_| {
                RpcError::internal_server_error().with_data("remote ledger RPC worker panicked")
            })?
    }
}
