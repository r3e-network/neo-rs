//! Strict JSON-RPC upstream adapter. No response field is parsed lossily.

use crate::config::{UpstreamConfig, UpstreamCredentials};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose};
use futures::StreamExt;
use neo_primitives::{Hardfork, UInt256};
use neo_protocol::ProtocolSettings;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    NeoCli,
    NeoGo,
}

impl NodeKind {
    pub const ALL: [Self; 2] = [Self::NeoCli, Self::NeoGo];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NeoCli => "neo_cli",
            Self::NeoGo => "neo_go",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionReceipt {
    pub trigger: String,
    pub vm_state: String,
    pub gas_consumed: i64,
    pub has_exception: bool,
}

#[derive(Debug, Clone)]
pub struct BlockEvidence {
    pub raw_block: Vec<u8>,
    pub block_hash: UInt256,
    pub state_root: UInt256,
    pub executions: Vec<ExecutionReceipt>,
}

#[async_trait]
pub trait NodeRpc: Send + Sync {
    fn kind(&self) -> NodeKind;
    async fn protocol_identity(&self) -> Result<Value>;
    async fn block_count(&self) -> Result<u32>;
    async fn block_evidence(&self, height: u32) -> Result<BlockEvidence>;
}

pub struct HttpNode {
    kind: NodeKind,
    client: reqwest::Client,
    url: Url,
    credentials: Option<UpstreamCredentials>,
    max_response_bytes: usize,
    max_block_bytes: usize,
    next_id: AtomicU64,
}

impl HttpNode {
    pub fn new(
        kind: NodeKind,
        config: &UpstreamConfig,
        timeout: Duration,
        max_response_bytes: usize,
        max_block_bytes: usize,
    ) -> Result<Arc<Self>> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .context("failed to construct upstream HTTP client")?;
        Ok(Arc::new(Self {
            kind,
            client,
            url: config.parsed_url()?,
            credentials: config.credentials()?,
            max_response_bytes,
            max_block_bytes,
            next_id: AtomicU64::new(1),
        }))
    }

    pub async fn send_raw(&self, body: &[u8]) -> Result<Vec<u8>> {
        let mut request = self
            .client
            .post(self.url.clone())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.to_vec());
        if let Some(credentials) = &self.credentials {
            request = request.basic_auth(
                credentials.username.as_str(),
                Some(credentials.password.as_str()),
            );
        }
        let response = request
            .send()
            .await
            .context("upstream HTTP request failed")?;
        let status = response.status();
        if let Some(length) = response.content_length()
            && length > self.max_response_bytes as u64
        {
            bail!("upstream response exceeds configured limit");
        }
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("failed while reading upstream response")?;
            if bytes.len().saturating_add(chunk.len()) > self.max_response_bytes {
                bail!("upstream response exceeds configured limit");
            }
            bytes.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            bail!("upstream returned HTTP {status}");
        }
        Ok(bytes)
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params});
        let bytes = self.send_raw(&serde_json::to_vec(&request)?).await?;
        let response: Value = serde_json::from_slice(&bytes).context("invalid upstream JSON")?;
        let object = response
            .as_object()
            .context("upstream response is not an object")?;
        if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
            || object.get("id") != Some(&json!(id))
        {
            bail!("upstream JSON-RPC envelope or id is invalid");
        }
        if let Some(error) = object.get("error").filter(|error| !error.is_null()) {
            let code = error.get("code").and_then(Value::as_i64).unwrap_or(-32603);
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown upstream error");
            bail!("upstream {method} failed with JSON-RPC {code}: {message}");
        }
        object
            .get("result")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("upstream {method} omitted result"))
    }
}

#[async_trait]
impl NodeRpc for HttpNode {
    fn kind(&self) -> NodeKind {
        self.kind
    }

    async fn protocol_identity(&self) -> Result<Value> {
        self.call("getversion", json!([])).await
    }

    async fn block_count(&self) -> Result<u32> {
        strict_u32(&self.call("getblockcount", json!([])).await?, "block count")
    }

    async fn block_evidence(&self, height: u32) -> Result<BlockEvidence> {
        let encoded = self.call("getblock", json!([height, false])).await?;
        let encoded = encoded
            .as_str()
            .context("getblock result is not a string")?;
        if encoded.len() > self.max_block_bytes.saturating_mul(2) {
            bail!("encoded block exceeds configured limit");
        }
        let raw_block = general_purpose::STANDARD
            .decode(encoded)
            .context("getblock result is not canonical Base64")?;
        if raw_block.len() > self.max_block_bytes {
            bail!("decoded block exceeds configured limit");
        }

        let hash = self.call("getblockhash", json!([height])).await?;
        let block_hash = parse_hash(hash.as_str(), "block hash")?;

        let root = self.call("getstateroot", json!([height])).await?;
        if strict_u32(
            root.get("index").context("state root index missing")?,
            "state index",
        )? != height
        {
            bail!("state root height does not match requested height");
        }
        let state_root = parse_hash(root.get("roothash").and_then(Value::as_str), "state root")?;

        let log = self
            .call("getapplicationlog", json!([block_hash.to_string()]))
            .await?;
        if let Some(log_hash) = log.get("blockhash")
            && parse_hash(log_hash.as_str(), "application-log block hash")? != block_hash
        {
            bail!("application log belongs to another block");
        }
        let executions = log
            .get("executions")
            .and_then(Value::as_array)
            .context("application log executions missing")?
            .iter()
            .map(parse_execution)
            .collect::<Result<Vec<_>>>()?;

        Ok(BlockEvidence {
            raw_block,
            block_hash,
            state_root,
            executions,
        })
    }
}

fn parse_execution(value: &Value) -> Result<ExecutionReceipt> {
    Ok(ExecutionReceipt {
        trigger: value
            .get("trigger")
            .and_then(Value::as_str)
            .context("execution trigger missing")?
            .to_owned(),
        vm_state: value
            .get("vmstate")
            .and_then(Value::as_str)
            .context("execution vmstate missing")?
            .to_owned(),
        gas_consumed: strict_i64(
            value.get("gasconsumed").context("execution gas missing")?,
            "gas consumed",
        )?,
        has_exception: value
            .get("exception")
            .and_then(Value::as_str)
            .is_some_and(|message| !message.is_empty()),
    })
}

fn parse_hash(value: Option<&str>, field: &str) -> Result<UInt256> {
    let value = value.with_context(|| format!("{field} is not a string"))?;
    UInt256::parse(value).map_err(|error| anyhow::anyhow!("invalid {field}: {error}"))
}

pub fn strict_u32(value: &Value, field: &str) -> Result<u32> {
    let number = value
        .as_u64()
        .with_context(|| format!("{field} is not an unsigned integer"))?;
    u32::try_from(number).with_context(|| format!("{field} exceeds u32"))
}

fn strict_i64(value: &Value, field: &str) -> Result<i64> {
    if let Some(number) = value.as_i64() {
        return Ok(number);
    }
    value
        .as_str()
        .with_context(|| format!("{field} is not an integer or decimal string"))?
        .parse()
        .with_context(|| format!("{field} is not a signed 64-bit integer"))
}

pub fn validate_protocol_identity(identity: &Value, settings: &ProtocolSettings) -> Result<()> {
    let protocol = identity
        .get("protocol")
        .and_then(Value::as_object)
        .context("getversion protocol object missing")?;
    let value = |field: &str| -> Result<&Value> {
        protocol
            .get(field)
            .with_context(|| format!("getversion protocol.{field} missing"))
    };
    let checks = [
        ("network", u64::from(settings.network)),
        ("addressversion", u64::from(settings.address_version)),
        (
            "maxtransactionsperblock",
            u64::from(settings.max_transactions_per_block),
        ),
        (
            "maxvaliduntilblockincrement",
            u64::from(settings.max_valid_until_block_increment),
        ),
        (
            "maxtraceableblocks",
            u64::from(settings.max_traceable_blocks),
        ),
    ];
    for (field, expected) in checks {
        let actual = value(field)?
            .as_u64()
            .with_context(|| format!("protocol.{field} is not an unsigned integer"))?;
        if actual != expected {
            bail!("protocol.{field}={actual} does not match expected {expected}");
        }
    }
    let block_time = protocol
        .get("msperblock")
        .or_else(|| protocol.get("millisecondsperblock"))
        .context("getversion protocol.msperblock missing")?
        .as_u64()
        .context("protocol.msperblock is not an unsigned integer")?;
    if block_time != u64::from(settings.milliseconds_per_block) {
        bail!(
            "protocol.msperblock={block_time} does not match expected {}",
            settings.milliseconds_per_block
        );
    }
    for (field, expected) in [
        ("validatorscount", i64::from(settings.validators_count)),
        (
            "memorypoolmaxtransactions",
            i64::from(settings.memory_pool_max_transactions),
        ),
        (
            "initialgasdistribution",
            settings.initial_gas_distribution as i64,
        ),
    ] {
        let actual = strict_i64(value(field)?, field)?;
        if actual != expected {
            bail!("protocol.{field}={actual} does not match expected {expected}");
        }
    }
    let committee = value("standbycommittee")?
        .as_array()
        .context("standbycommittee is not an array")?
        .iter()
        .map(|key| {
            key.as_str()
                .context("standby committee key is not a string")
                .map(str::to_owned)
        })
        .collect::<Result<Vec<_>>>()?;
    let expected_committee = settings
        .standby_committee
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if committee != expected_committee {
        bail!("standby committee does not match protocol settings");
    }
    let actual_hardforks = parse_hardforks(value("hardforks")?)?;
    let expected_hardforks = settings
        .hardforks
        .iter()
        .map(|(fork, height)| (*fork, *height))
        .collect::<std::collections::BTreeMap<_, _>>();
    if actual_hardforks != expected_hardforks {
        bail!(
            "hardfork configuration mismatch: actual={actual_hardforks:?}, expected={expected_hardforks:?}"
        );
    }
    Ok(())
}

fn parse_hardforks(value: &Value) -> Result<std::collections::BTreeMap<Hardfork, u32>> {
    let mut result = std::collections::BTreeMap::new();
    match value {
        Value::Object(entries) => {
            for (name, height) in entries {
                insert_hardfork(&mut result, name, height)?;
            }
        }
        Value::Array(entries) => {
            for entry in entries {
                let name = entry
                    .get("name")
                    .and_then(Value::as_str)
                    .context("hardfork name missing")?;
                let height = entry
                    .get("blockheight")
                    .context("hardfork blockheight missing")?;
                insert_hardfork(&mut result, name, height)?;
            }
        }
        _ => bail!("hardforks must be an object or an array"),
    }
    Ok(result)
}

fn insert_hardfork(
    result: &mut std::collections::BTreeMap<Hardfork, u32>,
    name: &str,
    height: &Value,
) -> Result<()> {
    let fork = name
        .parse::<Hardfork>()
        .map_err(|error| anyhow::anyhow!("unknown reported {error}"))?;
    let height = strict_u32(height, "hardfork block height")?;
    if result.insert(fork, height).is_some() {
        bail!("hardfork {name} is reported more than once");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(settings: &ProtocolSettings, go_shape: bool) -> Value {
        let hardforks = if go_shape {
            json!(
                settings
                    .hardforks
                    .iter()
                    .map(|(fork, height)| json!({"name": fork.name(), "blockheight": height}))
                    .collect::<Vec<_>>()
            )
        } else {
            json!(
                settings
                    .hardforks
                    .iter()
                    .map(|(fork, height)| (fork.name().to_owned(), *height))
                    .collect::<std::collections::BTreeMap<_, _>>()
            )
        };
        json!({
            "protocol": {
                "network": settings.network,
                "addressversion": settings.address_version,
                "msperblock": settings.milliseconds_per_block,
                "maxtransactionsperblock": settings.max_transactions_per_block,
                "maxvaliduntilblockincrement": settings.max_valid_until_block_increment,
                "maxtraceableblocks": settings.max_traceable_blocks,
                "validatorscount": settings.validators_count,
                "memorypoolmaxtransactions": settings.memory_pool_max_transactions,
                "initialgasdistribution": settings.initial_gas_distribution,
                "standbycommittee": settings.standby_committee.iter().map(ToString::to_string).collect::<Vec<_>>(),
                "hardforks": hardforks,
            }
        })
    }

    #[test]
    fn accepts_exact_neo_cli_and_neo_go_protocol_shapes() {
        let settings = ProtocolSettings::default();
        validate_protocol_identity(&identity(&settings, false), &settings).unwrap();
        validate_protocol_identity(&identity(&settings, true), &settings).unwrap();

        let mut legacy_time = identity(&settings, false);
        let protocol = legacy_time["protocol"].as_object_mut().unwrap();
        let milliseconds = protocol.remove("msperblock").unwrap();
        protocol.insert("millisecondsperblock".to_owned(), milliseconds);
        validate_protocol_identity(&legacy_time, &settings).unwrap();
    }

    #[test]
    fn rejects_duplicate_unknown_and_missing_hardforks() {
        let settings = ProtocolSettings::default();
        let mut duplicate = identity(&settings, true);
        let entries = duplicate["protocol"]["hardforks"].as_array_mut().unwrap();
        entries.push(entries[0].clone());
        assert!(
            validate_protocol_identity(&duplicate, &settings)
                .unwrap_err()
                .to_string()
                .contains("more than once")
        );

        let mut unknown = identity(&settings, true);
        unknown["protocol"]["hardforks"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name": "HF_Unknown", "blockheight": 0}));
        assert!(validate_protocol_identity(&unknown, &settings).is_err());

        let mut missing = identity(&settings, false);
        let hardforks = missing["protocol"]["hardforks"].as_object_mut().unwrap();
        let first = hardforks.keys().next().cloned().unwrap();
        hardforks.remove(&first);
        assert!(
            validate_protocol_identity(&missing, &settings)
                .unwrap_err()
                .to_string()
                .contains("hardfork configuration mismatch")
        );
    }
}
