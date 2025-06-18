//! Neo N3 RPC Client Library
//!
//! This library provides a complete RPC client implementation for Neo N3 blockchain,
//! matching the C# Neo.Network.RPC library exactly.
//!
//! ## Features
//!
//! - **Complete RPC API Coverage**: All Neo N3 RPC methods
//! - **Type Safety**: Strongly typed request/response objects
//! - **Async/Await Support**: Full async support with tokio
//! - **Error Handling**: Comprehensive error types
//! - **Connection Management**: Automatic retry and connection pooling
//! - **C# Compatibility**: Exact API compatibility with C# implementation

pub mod client;
pub mod error;
pub mod models;
pub mod methods;
pub mod neo_rpc;

// Re-export main types for convenience
pub use client::{RpcClient, RpcClientBuilder};
pub use error::{RpcError, RpcResult};
pub use models::*;

use serde::{Deserialize, Serialize};
use std::fmt;

/// RPC client configuration (matches C# RpcClient settings exactly)
#[derive(Debug, Clone)]
pub struct RpcConfig {
    /// RPC server endpoint URL
    pub endpoint: String,
    /// Request timeout in seconds
    pub timeout: u64,
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Retry delay in milliseconds
    pub retry_delay: u64,
    /// User agent string for HTTP requests
    pub user_agent: String,
    /// Custom HTTP headers
    pub headers: std::collections::HashMap<String, String>,
}

impl Default for RpcConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:10332".to_string(),
            timeout: 30,
            max_retries: 3,
            retry_delay: 1000,
            user_agent: "neo-rpc-client/0.1.0".to_string(),
            headers: std::collections::HashMap::new(),
        }
    }
}

/// JSON-RPC request structure (matches C# JsonRpc exactly)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    /// JSON-RPC version (always "2.0")
    pub jsonrpc: String,
    /// Request method name
    pub method: String,
    /// Request parameters
    pub params: serde_json::Value,
    /// Request ID
    pub id: u64,
}

impl JsonRpcRequest {
    /// Creates a new JSON-RPC request
    pub fn new(method: String, params: serde_json::Value, id: u64) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method,
            params,
            id,
        }
    }
}

/// JSON-RPC response structure (matches C# JsonRpc exactly)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    /// JSON-RPC version
    pub jsonrpc: String,
    /// Response result (if successful)
    pub result: Option<serde_json::Value>,
    /// Response error (if failed)
    pub error: Option<JsonRpcError>,
    /// Request ID
    pub id: u64,
}

/// JSON-RPC error structure (matches C# JsonRpcError exactly)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    /// Error code
    pub code: i32,
    /// Error message
    pub message: String,
    /// Additional error data
    pub data: Option<serde_json::Value>,
}

impl fmt::Display for JsonRpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RPC Error {}: {}", self.code, self.message)
    }
}

impl std::error::Error for JsonRpcError {}

/// RPC method names (matches C# RpcMethods exactly)
pub mod rpc_methods {
    /// Blockchain methods
    pub const GET_BEST_BLOCK_HASH: &str = "getbestblockhash";
    pub const GET_BLOCK: &str = "getblock";
    pub const GET_BLOCK_COUNT: &str = "getblockcount";
    pub const GET_BLOCK_HASH: &str = "getblockhash";
    pub const GET_BLOCK_HEADER: &str = "getblockheader";
    pub const GET_BLOCK_HEADER_COUNT: &str = "getblockheadercount";
    pub const GET_COMMITTEE: &str = "getcommittee";
    pub const GET_CONNECTION_COUNT: &str = "getconnectioncount";
    pub const GET_CONTRACT_STATE: &str = "getcontractstate";
    pub const GET_NATIVE_CONTRACTS: &str = "getnativecontracts";
    pub const GET_NEXT_BLOCK_VALIDATORS: &str = "getnextblockvalidators";
    pub const GET_PEERS: &str = "getpeers";
    pub const GET_RAW_MEMPOOL: &str = "getrawmempool";
    pub const GET_RAW_TRANSACTION: &str = "getrawtransaction";
    pub const GET_STORAGE: &str = "getstorage";
    pub const GET_TRANSACTION_HEIGHT: &str = "gettransactionheight";
    pub const GET_VALIDATORS: &str = "getvalidators";
    pub const GET_VERSION: &str = "getversion";

    /// Wallet methods
    pub const CALCULATE_NETWORK_FEE: &str = "calculatenetworkfee";
    pub const GET_APPLICATION_LOG: &str = "getapplicationlog";
    pub const GET_BALANCE: &str = "getbalance";
    pub const GET_NEP17_BALANCES: &str = "getnep17balances";
    pub const GET_NEP17_TRANSFERS: &str = "getnep17transfers";
    pub const GET_NEW_ADDRESS: &str = "getnewaddress";
    pub const GET_UNCLAIMED_GAS: &str = "getunclaimedgas";
    pub const GET_WALLET_BALANCE: &str = "getwalletbalance";
    pub const GET_WALLET_UNCLAIMED_GAS: &str = "getwalletunclaimedgas";
    pub const IMPORT_PRIVATE_KEY: &str = "importprivkey";
    pub const LIST_ADDRESS: &str = "listaddress";
    pub const SEND_FROM: &str = "sendfrom";
    pub const SEND_MANY: &str = "sendmany";
    pub const SEND_RAW_TRANSACTION: &str = "sendrawtransaction";
    pub const SEND_TO_ADDRESS: &str = "sendtoaddress";

    /// Smart contract methods
    pub const INVOKE_FUNCTION: &str = "invokefunction";
    pub const INVOKE_SCRIPT: &str = "invokescript";
    pub const TEST_INVOKE: &str = "testinvoke";

    /// Utility methods
    pub const LIST_PLUGINS: &str = "listplugins";
    pub const VALIDATE_ADDRESS: &str = "validateaddress";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rpc_config_default() {
        let config = RpcConfig::default();
        assert_eq!(config.endpoint, "http://localhost:10332");
        assert_eq!(config.timeout, 30);
        assert_eq!(config.max_retries, 3);
    }

    #[test]
    fn test_json_rpc_request() {
        let request = JsonRpcRequest::new(
            "getblockcount".to_string(),
            serde_json::Value::Array(vec![]),
            1,
        );
        assert_eq!(request.jsonrpc, "2.0");
        assert_eq!(request.method, "getblockcount");
        assert_eq!(request.id, 1);
    }

    #[test]
    fn test_json_rpc_serialization() {
        let request = JsonRpcRequest::new(
            "getblock".to_string(),
            serde_json::json!(["0x1234", true]),
            42,
        );

        let json = serde_json::to_string(&request).unwrap();
        let deserialized: JsonRpcRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(request.method, deserialized.method);
        assert_eq!(request.id, deserialized.id);
    }
} 