//! Minimal JSON-RPC client over HTTP to a running Neo node.
//!
//! Blocking (`reqwest::blocking`) — all calls run on worker threads, never on
//! the UI thread.

use std::time::Duration;

use anyhow::{anyhow, Context as _, Result};
use serde::Deserialize;
use serde_json::{json, Value};

/// A JSON-RPC client bound to one node endpoint.
#[derive(Clone)]
pub struct RpcClient {
    url: String,
    http: reqwest::blocking::Client,
}

impl RpcClient {
    /// Create a client for the given `http://host:port` endpoint.
    pub fn new(url: impl Into<String>) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .context("build reqwest client")?;
        Ok(Self {
            url: url.into(),
            http,
        })
    }

    /// Invoke a JSON-RPC method, returning the `result` value.
    pub fn call(&self, method: &str, params: Value) -> Result<Value> {
        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        });
        let resp: Value = self
            .http
            .post(&self.url)
            .json(&body)
            .send()
            .with_context(|| format!("POST {}", self.url))?
            .json()
            .context("decode JSON-RPC response")?;

        if let Some(err) = resp.get("error") {
            if !err.is_null() {
                let msg = err
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error");
                return Err(anyhow!("RPC error: {msg}"));
            }
        }
        resp.get("result")
            .cloned()
            .ok_or_else(|| anyhow!("response has no result"))
    }

    /// Fetch the headline status used by the dashboard in one round of calls.
    pub fn status(&self) -> Result<NodeStatus> {
        let version: Version =
            serde_json::from_value(self.call("getversion", json!([]))?).context("getversion")?;
        let block_count = self
            .call("getblockcount", json!([]))?
            .as_u64()
            .unwrap_or_default();
        let header_count = self
            .call("getblockheadercount", json!([]))
            .ok()
            .and_then(|v| v.as_u64())
            .unwrap_or(block_count);
        let connections = self
            .call("getconnectioncount", json!([]))?
            .as_u64()
            .unwrap_or_default();
        let best_hash = self
            .call("getbestblockhash", json!([]))?
            .as_str()
            .unwrap_or_default()
            .to_string();
        let mempool = self
            .call("getrawmempool", json!([]))
            .ok()
            .and_then(|v| v.as_array().map(|a| a.len()))
            .unwrap_or_default();

        Ok(NodeStatus {
            version,
            block_count,
            header_count,
            connections,
            best_hash,
            mempool,
        })
    }

    /// Fetch the connected/connecting peer lists.
    pub fn peers(&self) -> Result<Peers> {
        serde_json::from_value(self.call("getpeers", json!([]))?).context("getpeers")
    }
}

/// Headline node status for the dashboard.
#[derive(Clone, Debug, Default)]
pub struct NodeStatus {
    pub version: Version,
    pub block_count: u64,
    pub header_count: u64,
    pub connections: u64,
    pub best_hash: String,
    pub mempool: usize,
}

/// Decoded `getversion` result (only the fields the GUI shows).
#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default)]
pub struct Version {
    pub useragent: String,
    pub protocol: Protocol,
}

/// The `protocol` object inside `getversion`.
#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default)]
pub struct Protocol {
    pub network: u64,
    pub addressversion: u64,
    pub validatorscount: u64,
    pub msperblock: u64,
}

impl Protocol {
    /// Human network name for the well-known magics.
    pub fn network_name(&self) -> &'static str {
        match self.network {
            860_833_102 => "MainNet",
            894_710_606 => "TestNet",
            _ => "Private",
        }
    }
}

/// Decoded `getpeers` result.
#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default)]
pub struct Peers {
    pub connected: Vec<Peer>,
    pub unconnected: Vec<Peer>,
    pub bad: Vec<Peer>,
}

/// One peer entry.
#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default)]
pub struct Peer {
    pub address: String,
    pub port: u64,
}
