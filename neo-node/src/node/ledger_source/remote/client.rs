//! Blocking JSON-RPC client helpers for remote-ledger mode.

use std::sync::LazyLock;
use std::thread;
use std::time::Duration;

use anyhow::Context;
use serde_json::{Value, json};

const REMOTE_LEDGER_RPC_TIMEOUT: Duration = Duration::from_secs(15);
static REMOTE_LEDGER_HTTP_CLIENT: LazyLock<Result<reqwest::blocking::Client, String>> =
    LazyLock::new(|| build_remote_ledger_http_client(REMOTE_LEDGER_RPC_TIMEOUT));

fn build_remote_ledger_http_client(timeout: Duration) -> Result<reqwest::blocking::Client, String> {
    thread::spawn(move || {
        reqwest::blocking::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|err| err.to_string())
    })
    .join()
    .map_err(|_| "remote ledger RPC HTTP client builder panicked".to_string())?
}

fn remote_ledger_http_client() -> anyhow::Result<&'static reqwest::blocking::Client> {
    match &*REMOTE_LEDGER_HTTP_CLIENT {
        Ok(client) => Ok(client),
        Err(err) => anyhow::bail!("building remote ledger RPC HTTP client: {err}"),
    }
}

pub(super) fn rpc_call_blocking(
    endpoint: String,
    method: String,
    params: Vec<Value>,
) -> anyhow::Result<Value> {
    let client = remote_ledger_http_client()?;
    let response = client
        .post(&endpoint)
        .json(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": 1,
        }))
        .send()
        .with_context(|| format!("calling remote ledger RPC {method}"))?
        .error_for_status()
        .with_context(|| format!("remote ledger RPC {method} returned HTTP error"))?;
    let value: Value = response
        .json()
        .with_context(|| format!("decoding remote ledger RPC {method} response"))?;
    if let Some(error) = value.get("error") {
        anyhow::bail!("remote ledger RPC {method} returned error: {error}");
    }
    value
        .get("result")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("remote ledger RPC {method} response missing result"))
}
