//! # neo-rpc::server::rpc_remote_ledger
//!
//! Remote-ledger RPC client used by RPC-only node mode.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `rpc_remote_ledger`: remote-ledger RPC client and response mapping.

use std::sync::LazyLock;
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use super::rpc_error::RpcError;

const REMOTE_LEDGER_RPC_TIMEOUT: Duration = Duration::from_secs(15);
static REMOTE_LEDGER_HTTP_CLIENT: LazyLock<Result<reqwest::blocking::Client, String>> =
    LazyLock::new(|| build_blocking_http_client(REMOTE_LEDGER_RPC_TIMEOUT));

fn build_blocking_http_client(timeout: Duration) -> Result<reqwest::blocking::Client, String> {
    thread::spawn(move || {
        reqwest::blocking::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|err| err.to_string())
    })
    .join()
    .map_err(|_| "remote ledger RPC client builder panicked".to_string())?
}

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

/// Methods that should be delegated to the upstream RPC endpoint when the node
/// runs in remote-ledger mode.
///
/// Local node-control methods (`getversion`, `getpeers`, wallet management)
/// stay local because they describe or mutate this process. Ledger, state,
/// relay, invocation, session, indexer, token-tracker, plugin/service
/// inventory, wallet transaction-building, and oracle submission methods are
/// proxied so an RPC-ledger node does not answer from its ephemeral local chain
/// context.
#[must_use]
pub fn should_proxy_remote_ledger_method(method: &str) -> bool {
    matches!(
        method.to_ascii_lowercase().as_str(),
        "calculatenetworkfee"
            | "canceltransaction"
            | "findstates"
            | "findstorage"
            | "getcandidates"
            | "getcommittee"
            | "getaddressnotifications"
            | "getaddresstransactions"
            | "getapplicationlog"
            | "getbestblockhash"
            | "getblock"
            | "getblockcount"
            | "getblockhash"
            | "getblockheader"
            | "getblockheadercount"
            | "getblockindex"
            | "getblockindexes"
            | "getblocknotifications"
            | "getblocksysfee"
            | "getblocktransactions"
            | "getcontractnotifications"
            | "getcontractstate"
            | "getcontracttransactions"
            | "getindexerstatus"
            | "getnativecontracts"
            | "getnep11balances"
            | "getnep11properties"
            | "getnep11transfers"
            | "getnep17balances"
            | "getnep17transfers"
            | "getnextblockvalidators"
            | "getproof"
            | "getrawmempool"
            | "getrawtransaction"
            | "getstate"
            | "getstateheight"
            | "getstateroot"
            | "getstorage"
            | "gettransactionindex"
            | "gettransactionheight"
            | "gettransactionnotifications"
            | "getunclaimedgas"
            | "getwalletbalance"
            | "getwalletunclaimedgas"
            | "invokecontractverify"
            | "invokefunction"
            | "invokescript"
            | "listplugins"
            | "listservices"
            | "sendfrom"
            | "sendmany"
            | "sendrawtransaction"
            | "sendtoaddress"
            | "submitblock"
            | "submitoracleresponse"
            | "terminatesession"
            | "traverseiterator"
            | "verifyproof"
    )
}

fn call_remote_ledger_blocking(
    endpoint: String,
    method: String,
    params: Vec<Value>,
) -> Result<Value, RpcError> {
    let client = remote_http_client()?;
    let response = client
        .post(&endpoint)
        .json(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": 1,
        }))
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|err| {
            RpcError::internal_server_error()
                .with_data(format!("remote ledger RPC {method} request failed: {err}"))
        })?;
    let value: Value = response.json().map_err(|err| {
        RpcError::internal_server_error().with_data(format!(
            "remote ledger RPC {method} response was invalid: {err}"
        ))
    })?;
    if let Some(error) = value.get("error") {
        return Err(RpcError::internal_server_error().with_data(format!(
            "remote ledger RPC {method} returned error: {error}"
        )));
    }
    value.get("result").cloned().ok_or_else(|| {
        RpcError::internal_server_error().with_data(format!(
            "remote ledger RPC {method} response missing result"
        ))
    })
}

fn remote_http_client() -> Result<&'static reqwest::blocking::Client, RpcError> {
    match &*REMOTE_LEDGER_HTTP_CLIENT {
        Ok(client) => Ok(client),
        Err(err) => Err(RpcError::internal_server_error()
            .with_data(format!("failed to build remote ledger RPC client: {err}"))),
    }
}
