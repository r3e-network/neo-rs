//! Remote JSON-RPC implementation of [`neo_network::BlockSource`].

use std::sync::Arc;
use std::thread;

use anyhow::Context;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_io::{MemoryReader, Serializable};
use neo_payloads::Transaction;
use neo_primitives::UInt256;
use serde_json::{Value, json};
use tracing::warn;

use super::client::rpc_call_blocking;
use super::payload::parse_remote_mempool_hashes;
use crate::node::rpc_payload::decode_remote_serialized_payload;

/// Read-only ledger view backed by a remote JSON-RPC endpoint.
///
/// This is intentionally limited to historical block/header lookup and does not
/// replace the local execution/state provider used for block validation.
pub(in crate::node) struct RpcLedgerBlockSource {
    endpoint: String,
    mempool: Arc<neo_mempool::MemoryPool>,
}

impl RpcLedgerBlockSource {
    pub(in crate::node) fn new(
        endpoint: impl Into<String>,
        mempool: Arc<neo_mempool::MemoryPool>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            endpoint: endpoint.into(),
            mempool,
        })
    }

    pub(in crate::node) fn remote_tip_height(&self) -> anyhow::Result<u32> {
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
