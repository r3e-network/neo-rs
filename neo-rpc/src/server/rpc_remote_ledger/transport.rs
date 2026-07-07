//! Blocking HTTP transport for remote-ledger JSON-RPC calls.

use std::sync::LazyLock;
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use crate::server::rpc_error::RpcError;

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

pub(super) fn call_remote_ledger_blocking(
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
