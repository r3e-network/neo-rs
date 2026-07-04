//! # neo-node::node::ledger_source
//!
//! Local and remote ledger source abstractions used by node modes.
//!
//! ## Boundary
//!
//! This module belongs to `neo-node`. This application crate may compose lower
//! layers but must not define protocol bytes, storage formats, consensus rules,
//! or VM semantics.
//!
//! ## Contents
//!
//! - `ledger_source`: block-source trait and RPC-backed source implementation.

use std::sync::{Arc, LazyLock};
use std::thread;
use std::time::Duration;

use anyhow::Context;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_io::{MemoryReader, Serializable};
use neo_payloads::Transaction;
use neo_primitives::{UInt256, hex_util};
use serde_json::{Value, json};
use tracing::warn;

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

/// Read-only ledger view that serves peers' block requests
/// ([`neo_network::BlockSource`]) by reconstructing a full block from the
/// persistent store: `index → hash → TrimmedBlock → transactions`
/// (the C# `NativeContract.Ledger.GetBlock(snapshot, index)` path).
pub(super) struct LedgerBlockSource {
    snapshot: Arc<neo_storage::persistence::DataCache>,
    /// Blockchain relay cache for accepted extensible payloads (dBFT and
    /// state-service messages).
    ledger: Arc<neo_blockchain::LedgerContext>,
    /// The shared mempool, so `Inv`/`Mempool` gossip can answer for
    /// unconfirmed transactions (which are not yet in the ledger snapshot).
    mempool: Arc<neo_mempool::MemoryPool>,
}

impl LedgerBlockSource {
    pub(super) fn new(
        snapshot: Arc<neo_storage::persistence::DataCache>,
        ledger: Arc<neo_blockchain::LedgerContext>,
        mempool: Arc<neo_mempool::MemoryPool>,
    ) -> Self {
        Self {
            snapshot,
            ledger,
            mempool,
        }
    }

    /// Reconstructs the full block stored under `hash`: header + the
    /// transactions referenced by its `TrimmedBlock`.
    fn full_block(
        &self,
        ledger: &neo_native_contracts::LedgerContract,
        hash: &neo_primitives::UInt256,
    ) -> Option<neo_payloads::Block> {
        let trimmed = ledger.get_trimmed_block(&self.snapshot, hash).ok()??;
        let mut transactions = Vec::with_capacity(trimmed.hashes.len());
        for tx_hash in &trimmed.hashes {
            let state = ledger
                .get_transaction_state(&self.snapshot, tx_hash)
                .ok()??;
            transactions.push(state.transaction?);
        }
        Some(neo_payloads::Block::from_parts(
            trimmed.header,
            transactions,
        ))
    }
}

impl neo_network::BlockSource for LedgerBlockSource {
    fn block_by_index(&self, index: u32) -> Option<neo_payloads::Block> {
        let ledger = neo_native_contracts::LedgerContract::new();
        let hash = ledger.get_block_hash(&self.snapshot, index).ok()??;
        self.full_block(&ledger, &hash)
    }

    fn header_by_index(&self, index: u32) -> Option<neo_payloads::Header> {
        let ledger = neo_native_contracts::LedgerContract::new();
        let hash = ledger.get_block_hash(&self.snapshot, index).ok()??;
        let trimmed = ledger.get_trimmed_block(&self.snapshot, &hash).ok()??;
        Some(trimmed.header)
    }

    fn block_hash_by_index(&self, index: u32) -> Option<neo_primitives::UInt256> {
        neo_native_contracts::LedgerContract::new()
            .get_block_hash(&self.snapshot, index)
            .ok()
            .flatten()
    }

    fn block_by_hash(&self, hash: &neo_primitives::UInt256) -> Option<neo_payloads::Block> {
        self.full_block(&neo_native_contracts::LedgerContract::new(), hash)
    }

    fn block_index_by_hash(&self, hash: &neo_primitives::UInt256) -> Option<u32> {
        neo_native_contracts::LedgerContract::new()
            .get_trimmed_block(&self.snapshot, hash)
            .ok()
            .flatten()
            .map(|trimmed| trimmed.header.index())
    }

    fn transaction_by_hash(
        &self,
        hash: &neo_primitives::UInt256,
    ) -> Option<neo_payloads::Transaction> {
        // Serve unconfirmed transactions from the mempool first (C# `GetData`
        // serves `MemoryPool` entries), then fall back to the ledger.
        if let Some(item) = self.mempool.get(hash) {
            return Some((*item.transaction).clone());
        }
        neo_native_contracts::LedgerContract::new()
            .get_transaction_state(&self.snapshot, hash)
            .ok()?
            .and_then(|state| state.transaction)
    }

    fn extensible_by_hash(
        &self,
        hash: &neo_primitives::UInt256,
    ) -> Option<neo_payloads::ExtensiblePayload> {
        self.ledger.get_extensible(hash)
    }

    fn contains_transaction(&self, hash: &neo_primitives::UInt256) -> bool {
        self.mempool.contains(hash)
            || neo_native_contracts::LedgerContract::new()
                .get_transaction_state(&self.snapshot, hash)
                .ok()
                .flatten()
                .is_some()
    }

    fn mempool_transaction_hashes(&self) -> Vec<neo_primitives::UInt256> {
        self.mempool
            .verified_snapshot()
            .iter()
            .map(|item| item.hash())
            .collect()
    }
}

/// Read-only ledger view backed by a remote JSON-RPC endpoint.
///
/// This is intentionally limited to historical block/header lookup and does not
/// replace the local execution/state provider used for block validation.
pub(super) struct RpcLedgerBlockSource {
    endpoint: String,
    mempool: Arc<neo_mempool::MemoryPool>,
}

impl RpcLedgerBlockSource {
    pub(super) fn new(
        endpoint: impl Into<String>,
        mempool: Arc<neo_mempool::MemoryPool>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            endpoint: endpoint.into(),
            mempool,
        })
    }

    pub(super) fn remote_tip_height(&self) -> anyhow::Result<u32> {
        let count = self.rpc_call("getblockcount", Vec::new())?;
        let count = count.as_u64().ok_or_else(|| {
            anyhow::anyhow!("remote ledger RPC getblockcount returned non-integer result")
        })?;
        if count == 0 {
            return Ok(0);
        }
        u32::try_from(count - 1)
            .map_err(|_| anyhow::anyhow!("remote ledger RPC getblockcount exceeds u32 height"))
    }

    fn rpc_call(&self, method: &str, params: Vec<Value>) -> anyhow::Result<Value> {
        let endpoint = self.endpoint.clone();
        let method = method.to_owned();
        thread::spawn(move || rpc_call_blocking(endpoint, method, params))
            .join()
            .map_err(|_| anyhow::anyhow!("remote ledger RPC worker panicked"))?
    }

    fn rpc_string(&self, method: &str, params: Vec<Value>) -> anyhow::Result<String> {
        let result = self.rpc_call(method, params)?;
        result
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("remote ledger RPC {method} returned non-string result"))
    }

    fn rpc_base64(&self, method: &str, params: Vec<Value>) -> anyhow::Result<Vec<u8>> {
        let encoded = self.rpc_string(method, params)?;
        BASE64_STANDARD
            .decode(encoded.trim())
            .context("decoding remote ledger base64 payload")
    }

    fn decode_block(raw_text: &str) -> anyhow::Result<neo_payloads::Block> {
        decode_remote_serialized_payload(raw_text, "block", |bytes| {
            neo_payloads::Block::deserialize(&mut MemoryReader::new(bytes))
                .map_err(|err| anyhow::anyhow!("deserializing remote ledger block: {err}"))
        })
    }

    fn decode_header(raw_text: &str) -> anyhow::Result<neo_payloads::Header> {
        decode_remote_serialized_payload(raw_text, "header", |bytes| {
            neo_payloads::Header::deserialize(&mut MemoryReader::new(bytes))
                .map_err(|err| anyhow::anyhow!("deserializing remote ledger header: {err}"))
        })
    }

    fn block_by_selector(&self, selector: Value) -> Option<neo_payloads::Block> {
        self.rpc_string("getblock", vec![selector])
            .and_then(|hex| Self::decode_block(&hex))
            .map_err(|err| {
                warn!(target: "neo::remote_ledger", error = %err, "remote getblock failed");
                err
            })
            .ok()
    }

    fn header_by_selector(&self, selector: Value) -> Option<neo_payloads::Header> {
        self.rpc_string("getblockheader", vec![selector])
            .and_then(|hex| Self::decode_header(&hex))
            .map_err(|err| {
                warn!(target: "neo::remote_ledger", error = %err, "remote getblockheader failed");
                err
            })
            .ok()
    }

    fn remote_mempool_transaction_hashes(&self) -> anyhow::Result<Vec<UInt256>> {
        let value = self.rpc_call("getrawmempool", Vec::new())?;
        parse_remote_mempool_hashes(value)
    }
}

fn decode_remote_serialized_payload<T>(
    raw_text: &str,
    label: &'static str,
    deserialize: impl Fn(&[u8]) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let trimmed = raw_text.trim();
    let prefer_hex = looks_like_hex_payload(trimmed);
    let first = if prefer_hex {
        RemotePayloadEncoding::Hex
    } else {
        RemotePayloadEncoding::Base64
    };
    let second = if prefer_hex {
        RemotePayloadEncoding::Base64
    } else {
        RemotePayloadEncoding::Hex
    };

    let first_error = match decode_remote_serialized_with(trimmed, label, first, &deserialize) {
        Ok(payload) => return Ok(payload),
        Err(err) => err,
    };
    match decode_remote_serialized_with(trimmed, label, second, &deserialize) {
        Ok(payload) => Ok(payload),
        Err(second_error) => Err(anyhow::anyhow!(
            "remote ledger {label} was neither valid {first} nor {second}: {first_error}; {second_error}"
        )),
    }
}

fn decode_remote_serialized_with<T>(
    text: &str,
    label: &'static str,
    encoding: RemotePayloadEncoding,
    deserialize: impl Fn(&[u8]) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let bytes = match encoding {
        RemotePayloadEncoding::Base64 => BASE64_STANDARD
            .decode(text)
            .with_context(|| format!("decoding remote ledger {label} base64"))?,
        RemotePayloadEncoding::Hex => {
            let hex_text = text.strip_prefix("0x").unwrap_or(text);
            hex_util::decode_hex(hex_text).with_context(|| format!("decoding remote ledger {label} hex"))?
        }
    };
    deserialize(&bytes)
}

#[derive(Clone, Copy)]
enum RemotePayloadEncoding {
    Base64,
    Hex,
}

impl std::fmt::Display for RemotePayloadEncoding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Base64 => f.write_str("base64"),
            Self::Hex => f.write_str("hex"),
        }
    }
}

fn looks_like_hex_payload(text: &str) -> bool {
    let hex_text = text.strip_prefix("0x").unwrap_or(text);
    !hex_text.is_empty()
        && hex_text.len() % 2 == 0
        && hex_text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn rpc_call_blocking(
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

impl neo_network::BlockSource for RpcLedgerBlockSource {
    fn block_by_index(&self, index: u32) -> Option<neo_payloads::Block> {
        self.block_by_selector(json!(index))
    }

    fn header_by_index(&self, index: u32) -> Option<neo_payloads::Header> {
        self.header_by_selector(json!(index))
    }

    fn block_hash_by_index(&self, index: u32) -> Option<UInt256> {
        self.rpc_string("getblockhash", vec![json!(index)])
            .and_then(|hash| UInt256::parse(&hash).map_err(|err| anyhow::anyhow!("{err}")))
            .map_err(|err| {
                warn!(target: "neo::remote_ledger", error = %err, "remote getblockhash failed");
                err
            })
            .ok()
    }

    fn block_by_hash(&self, hash: &UInt256) -> Option<neo_payloads::Block> {
        self.block_by_selector(json!(hash.to_string()))
    }

    fn block_index_by_hash(&self, hash: &UInt256) -> Option<u32> {
        self.header_by_selector(json!(hash.to_string()))
            .map(|header| header.index())
    }

    fn transaction_by_hash(&self, hash: &UInt256) -> Option<neo_payloads::Transaction> {
        if let Some(item) = self.mempool.get(hash) {
            return Some((*item.transaction).clone());
        }
        self.rpc_base64("getrawtransaction", vec![json!(hash.to_string())])
            .and_then(|bytes| {
                Transaction::deserialize(&mut MemoryReader::new(&bytes)).map_err(|err| {
                    anyhow::anyhow!("deserializing remote ledger transaction: {err}")
                })
            })
            .map_err(|err| {
                warn!(
                    target: "neo::remote_ledger",
                    error = %err,
                    "remote getrawtransaction failed"
                );
                err
            })
            .ok()
    }

    fn contains_transaction(&self, hash: &UInt256) -> bool {
        self.mempool.contains(hash) || self.transaction_by_hash(hash).is_some()
    }

    fn mempool_transaction_hashes(&self) -> Vec<UInt256> {
        match self.remote_mempool_transaction_hashes() {
            Ok(hashes) => hashes,
            Err(err) => {
                warn!(
                    target: "neo::remote_ledger",
                    error = %err,
                    "remote getrawmempool failed"
                );
                self.mempool
                    .verified_snapshot()
                    .iter()
                    .map(|item| item.hash())
                    .collect()
            }
        }
    }
}

fn parse_remote_mempool_hashes(value: Value) -> anyhow::Result<Vec<UInt256>> {
    let hashes = if let Some(array) = value.as_array() {
        array
    } else if let Some(array) = value.get("verified").and_then(Value::as_array) {
        array
    } else {
        anyhow::bail!("remote ledger RPC getrawmempool returned non-array result");
    };
    hashes
        .iter()
        .map(|value| {
            let hash = value.as_str().ok_or_else(|| {
                anyhow::anyhow!("remote ledger RPC getrawmempool returned non-string hash")
            })?;
            UInt256::parse(hash).map_err(|err| anyhow::anyhow!("{err}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_io::BinaryWriter;
    use neo_network::BlockSource;
    use neo_payloads::{Signer, Witness};
    use neo_primitives::{UInt160, WitnessScope};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn empty_block(index: u32) -> neo_payloads::Block {
        let mut header = neo_payloads::Header::new();
        header.set_index(index);
        neo_payloads::Block::from_parts(header, Vec::new())
    }

    fn test_transaction(nonce: u32) -> neo_payloads::Transaction {
        let mut tx = neo_payloads::Transaction::new();
        tx.set_version(0);
        tx.set_nonce(nonce);
        tx.set_system_fee(0);
        tx.set_network_fee(0);
        tx.set_valid_until_block(1);
        tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::NONE)]);
        tx.set_attributes(Vec::new());
        tx.set_script(vec![neo_vm_rs::OpCode::PUSH1.byte()]);
        tx.set_witnesses(vec![Witness::empty()]);
        tx
    }

    fn serialized_hex<T: Serializable>(payload: &T) -> String {
        let mut writer = BinaryWriter::new();
        payload.serialize(&mut writer).expect("serialize payload");
        hex::encode(writer.into_bytes())
    }

    fn serialized_base64<T: Serializable>(payload: &T) -> String {
        let mut writer = BinaryWriter::new();
        payload.serialize(&mut writer).expect("serialize payload");
        base64::engine::general_purpose::STANDARD.encode(writer.into_bytes())
    }

    fn test_mempool() -> Arc<neo_mempool::MemoryPool> {
        Arc::new(neo_mempool::MemoryPool::new(
            &neo_config::ProtocolSettings::default(),
        ))
    }

    fn serve_rpc_once(expected_method: &'static str, result: Value) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test RPC");
        let url = format!("http://{}", listener.local_addr().expect("addr"));
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let read = stream.read(&mut buf).expect("read request");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let text = String::from_utf8_lossy(&request);
            assert!(
                text.contains(&format!(r#""method":"{expected_method}""#))
                    || text.contains(&format!(r#""method": "{expected_method}""#)),
                "unexpected request: {text}"
            );
            let body = json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": result,
            })
            .to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("write response");
        });
        url
    }

    #[test]
    fn rpc_ledger_block_source_decodes_raw_block_by_index() {
        let block = empty_block(7);
        let url = serve_rpc_once("getblock", json!(serialized_hex(&block)));
        let source = RpcLedgerBlockSource::new(url, test_mempool()).expect("source");

        let fetched = source.block_by_index(7).expect("remote block");

        assert_eq!(fetched.index(), 7);
        assert_eq!(fetched.hash(), block.hash());
    }

    #[test]
    fn rpc_ledger_block_source_decodes_base64_raw_block_by_index() {
        let block = empty_block(17);
        let url = serve_rpc_once("getblock", json!(serialized_base64(&block)));
        let source = RpcLedgerBlockSource::new(url, test_mempool()).expect("source");

        let fetched = source.block_by_index(17).expect("remote block");

        assert_eq!(fetched.index(), 17);
        assert_eq!(fetched.hash(), block.hash());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn rpc_ledger_block_source_can_be_called_from_async_runtime() {
        let block = empty_block(11);
        let url = serve_rpc_once("getblock", json!(serialized_hex(&block)));
        let source = RpcLedgerBlockSource::new(url, test_mempool()).expect("source");

        let fetched = source.block_by_index(11).expect("remote block");

        assert_eq!(fetched.index(), 11);
        assert_eq!(fetched.hash(), block.hash());
    }

    #[test]
    fn rpc_ledger_block_source_decodes_raw_header_by_index() {
        let block = empty_block(9);
        let url = serve_rpc_once("getblockheader", json!(serialized_hex(&block.header)));
        let source = RpcLedgerBlockSource::new(url, test_mempool()).expect("source");

        let fetched = source.header_by_index(9).expect("remote header");

        assert_eq!(fetched.index(), 9);
        assert_eq!(fetched.hash(), block.header.hash());
    }

    #[test]
    fn rpc_ledger_block_source_decodes_base64_raw_header_by_index() {
        let block = empty_block(19);
        let url = serve_rpc_once("getblockheader", json!(serialized_base64(&block.header)));
        let source = RpcLedgerBlockSource::new(url, test_mempool()).expect("source");

        let fetched = source.header_by_index(19).expect("remote header");

        assert_eq!(fetched.index(), 19);
        assert_eq!(fetched.hash(), block.header.hash());
    }

    #[test]
    fn rpc_ledger_block_source_decodes_raw_transaction_by_hash() {
        let tx = test_transaction(91);
        let tx_hash = tx.try_hash().expect("tx hash");
        let url = serve_rpc_once("getrawtransaction", json!(serialized_base64(&tx)));
        let source = RpcLedgerBlockSource::new(url, test_mempool()).expect("source");

        let fetched = source
            .transaction_by_hash(&tx_hash)
            .expect("remote transaction");

        assert_eq!(fetched.try_hash().expect("fetched tx hash"), tx_hash);
    }

    #[test]
    fn rpc_ledger_block_source_contains_transaction_uses_remote_rpc() {
        let tx = test_transaction(117);
        let tx_hash = tx.try_hash().expect("tx hash");
        let url = serve_rpc_once("getrawtransaction", json!(serialized_base64(&tx)));
        let source = RpcLedgerBlockSource::new(url, test_mempool()).expect("source");

        assert!(
            source.contains_transaction(&tx_hash),
            "remote-ledger mode should treat upstream transactions as known"
        );
    }

    #[test]
    fn rpc_ledger_block_source_mempool_hashes_use_remote_rpc() {
        let tx_hash =
            UInt256::parse("0x0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20")
                .expect("tx hash");
        let url = serve_rpc_once("getrawmempool", json!([tx_hash.to_string()]));
        let source = RpcLedgerBlockSource::new(url, test_mempool()).expect("source");

        assert_eq!(source.mempool_transaction_hashes(), vec![tx_hash]);
    }

    #[test]
    fn rpc_ledger_block_source_mempool_hashes_accept_verbose_remote_shape() {
        let tx_hash =
            UInt256::parse("0x202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f")
                .expect("tx hash");
        let url = serve_rpc_once(
            "getrawmempool",
            json!({
                "height": 42,
                "verified": [tx_hash.to_string()],
                "unverified": [],
            }),
        );
        let source = RpcLedgerBlockSource::new(url, test_mempool()).expect("source");

        assert_eq!(source.mempool_transaction_hashes(), vec![tx_hash]);
    }

    #[test]
    fn rpc_ledger_block_source_returns_none_on_rpc_error() {
        let url = serve_rpc_once("getblock", Value::Null);
        let source = RpcLedgerBlockSource::new(url, test_mempool()).expect("source");

        assert!(source.block_by_index(1).is_none());
    }
}
