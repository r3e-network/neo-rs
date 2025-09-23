//! RPC Models
//!
//! This module contains data models for RPC requests and responses.

use neo_core::{UInt160, UInt256};
use neo_ledger::block::BlockHeader;
use neo_vm::VMState;
use serde::{Deserialize, Serialize};
use std::fmt;

/// RPC request structure (matches Neo C# RPC protocol)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub method: String,
    pub params: serde_json::Value,
    pub id: Option<serde_json::Value>,
}

/// JSON-RPC request structure (alias for RpcRequest)
pub type JsonRpcRequest = RpcRequest;

/// JSON-RPC response structure (alias for RpcResponse)
pub type JsonRpcResponse = RpcResponse;

/// RPC response structure (matches Neo C# RPC protocol)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    pub jsonrpc: String,
    pub result: Option<serde_json::Value>,
    pub error: Option<JsonRpcError>,
    pub id: Option<serde_json::Value>,
}

/// JSON-RPC error structure (matches Neo C# RPC protocol)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

impl fmt::Display for JsonRpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RPC Error {}: {}", self.code, self.message)
    }
}

impl std::error::Error for JsonRpcError {}

/// Block information for RPC responses (matches Neo C# RPC protocol)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcBlock {
    pub hash: UInt256,
    pub size: u32,
    pub version: u32,
    pub merkleroot: UInt256,
    pub time: u64,
    pub index: u32,
    pub primary: u8,
    pub nextconsensus: UInt160,
    pub witnesses: Vec<RpcWitness>,
    pub tx: Vec<RpcTransaction>,
    pub confirmations: Option<u32>,
    pub nextblockhash: Option<UInt256>,
}

/// Transaction information for RPC responses (matches Neo C# RPC protocol)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcTransaction {
    pub hash: UInt256,
    pub size: u32,
    pub version: u8,
    pub nonce: u32,
    pub sender: Option<UInt160>,
    pub sysfee: String,
    pub netfee: String,
    pub validuntilblock: u32,
    pub signers: Vec<RpcSigner>,
    pub attributes: Vec<RpcTransactionAttribute>,
    pub script: Vec<u8>,
    pub witnesses: Vec<RpcWitness>,
    pub blockhash: Option<UInt256>,
    pub confirmations: Option<u32>,
    pub blocktime: Option<u64>,
}

/// Witness information for RPC responses (matches Neo C# RPC protocol)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcWitness {
    pub invocation: String,
    pub verification: String,
}

/// Signer information for RPC responses (matches Neo C# RPC protocol)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcSigner {
    pub account: UInt160,
    pub scopes: String,
    pub allowedcontracts: Option<Vec<UInt160>>,
    pub allowedgroups: Option<Vec<String>>,
    pub rules: Option<Vec<RpcWitnessRule>>,
}

/// Witness rule for RPC responses (matches Neo C# RPC protocol)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcWitnessRule {
    pub action: String,
    pub condition: serde_json::Value,
}

/// Transaction attribute for RPC responses (matches Neo C# RPC protocol)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcTransactionAttribute {
    #[serde(rename = "type")]
    pub attr_type: String,
    #[serde(flatten)]
    pub data: serde_json::Value,
}

/// Application log for RPC responses (matches Neo C# RPC protocol)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcApplicationLog {
    pub txid: Option<UInt256>,
    pub blockhash: UInt256,
    pub executions: Vec<RpcExecution>,
}

/// Execution information for RPC responses (matches Neo C# RPC protocol)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcExecution {
    pub trigger: String,
    pub vmstate: String,
    pub exception: Option<String>,
    pub gasconsumed: String,
    pub stack: Vec<serde_json::Value>,
    pub tx: Option<String>,
    pub exception: Option<String>,
    pub session: Option<String>,
    pub notifications: Vec<RpcNotification>,
}

/// Notification information for RPC responses (matches Neo C# RPC protocol)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcNotification {
    pub contract: UInt160,
    pub eventname: String,
    pub state: serde_json::Value,
}

/// NEP-17 token balance information (matches Neo C# RPC protocol)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcNep17Balance {
    pub assethash: UInt160,
    pub amount: String,
    pub lastupdatedblock: u32,
}

/// NEP-11 token information (matches Neo C# RPC protocol)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcNep11Token {
    pub assethash: UInt160,
    pub tokenid: String,
    pub amount: String,
    pub lastupdatedblock: u32,
}

/// Validator information for RPC responses (matches Neo C# RPC protocol)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcValidator {
    pub publickey: String,
    pub votes: String,
    pub active: bool,
}

/// Peers information for RPC responses (matches Neo C# RPC protocol)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcPeers {
    pub unconnected: Vec<RpcPeer>,
    pub bad: Vec<RpcPeer>,
    pub connected: Vec<RpcPeer>,
}

/// Peer information for RPC responses (matches Neo C# RPC protocol)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcPeer {
    pub address: String,
    pub port: u16,
}

/// Version information for RPC responses (matches Neo C# RPC protocol)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcVersion {
    pub tcpport: u16,
    pub wsport: u16,
    pub nonce: u32,
    pub useragent: String,
    pub protocol: RpcProtocolConfiguration,
}

/// Protocol configuration for RPC responses (matches Neo C# RPC protocol)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcProtocolConfiguration {
    pub addressversion: u8,
    pub network: u32,
    pub validatorscount: u8,
    pub msperblock: u32,
    pub maxtraceableblocks: u32,
    pub maxvaliduntilblockincrement: u32,
    pub maxtransactionsperblock: u32,
    pub memorypoolmaxtransactions: u32,
    pub initialgasdistribution: String,
}

impl RpcRequest {
    /// Creates a new RPC request
    pub fn new(method: String, params: serde_json::Value, id: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method,
            params,
            id,
        }
    }
}

impl RpcResponse {
    /// Creates a successful RPC response
    pub fn success(result: serde_json::Value, id: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: Some(result),
            error: None,
            id,
        }
    }

    /// Creates an error RPC response
    pub fn error(error: JsonRpcError, id: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(error),
            id,
        }
    }
}

impl JsonRpcError {
    /// Creates a new RPC error
    pub fn new(code: i32, message: String) -> Self {
        Self {
            code,
            message,
            data: None,
        }
    }

    /// Creates an RPC error with additional data
    pub fn with_data(code: i32, message: String, data: serde_json::Value) -> Self {
        Self {
            code,
            message,
            data: Some(data),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcAccount {
    pub address: String,
    #[serde(rename = "haskey")]
    pub has_key: bool,
    pub label: Option<String>,
    #[serde(rename = "watchonly")]
    pub watch_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcPlugin {
    pub name: String,
    pub version: String,
    pub interfaces: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcBlockHeader {
    pub header: neo_ledger::block::BlockHeader,
    pub confirmations: u32,
    #[serde(rename = "nextblockhash")]
    pub next_block_hash: Option<UInt256>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcContractState {
    pub id: i32,
    pub updatecounter: u16,
    pub hash: UInt160,
    pub nef: serde_json::Value,
    pub manifest: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcFoundStates {
    pub truncated: bool,
    pub results: Vec<RpcFoundStateEntry>,
    #[serde(rename = "firstProof")]
    pub first_proof: Option<Vec<u8>>,
    #[serde(rename = "lastProof")]
    pub last_proof: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcFoundStateEntry {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcInvokeResult {
    pub script: String,
    pub state: VMState,
    #[serde(rename = "gasconsumed")]
    pub gas_consumed: i64,
    pub stack: Vec<serde_json::Value>,
    pub tx: Option<String>,
    pub exception: Option<String>,
    pub session: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcMethodToken {
    pub hash: UInt160,
    pub method: String,
    #[serde(rename = "paramcount")]
    pub parameters_count: u16,
    #[serde(rename = "hasreturnvalue")]
    pub has_return_value: bool,
    #[serde(rename = "callflags")]
    pub call_flags: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcNefFile {
    pub compiler: String,
    pub source: Option<String>,
    pub tokens: Vec<RpcMethodToken>,
    pub script: Vec<u8>,
    #[serde(rename = "checksum")]
    pub check_sum: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcStateRoot {
    pub version: u8,
    pub index: u32,
    #[serde(rename = "roothash")]
    pub root_hash: UInt256,
    pub witness: Option<Witness>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRawMemPool {
    pub height: u32,
    pub verified: Vec<UInt256>,
    #[serde(rename = "unverified")]
    pub unverified: Vec<UInt256>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcValidateAddressResult {
    pub address: String,
    #[serde(rename = "isvalid")]
    pub is_valid: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcTransferOut {
    pub asset: UInt160,
    #[serde(rename = "address")]
    pub script_hash: UInt160,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcNep17Balances {
    #[serde(rename = "address")]
    pub user_script_hash: UInt160,
    pub balance: Vec<RpcNep17Balance>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcNep17Transfers {
    #[serde(rename = "address")]
    pub user_script_hash: UInt160,
    pub sent: Vec<RpcNep17Transfer>,
    pub received: Vec<RpcNep17Transfer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcNep17Transfer {
    #[serde(rename = "timestamp")]
    pub timestamp_ms: u64,
    #[serde(rename = "assethash")]
    pub asset_hash: UInt160,
    #[serde(rename = "transferaddress")]
    pub transfer_address: Option<UInt160>,
    pub amount: String,
    #[serde(rename = "blockindex")]
    pub block_index: u32,
    #[serde(rename = "transfernotifyindex")]
    pub transfer_notify_index: u16,
    #[serde(rename = "txhash")]
    pub tx_hash: UInt256,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcNep17TokenInfo {
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    #[serde(rename = "totalsupply")]
    pub total_supply: String,
}
