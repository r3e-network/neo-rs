//! RPC server configuration.

use serde::{Deserialize, Serialize};

/// RPC server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcSettings {
    /// Enable RPC server
    #[serde(default = "default_rpc_enabled")]
    pub enabled: bool,

    /// RPC listen address
    #[serde(default = "default_rpc_address")]
    pub address: String,

    /// RPC port
    #[serde(default = "default_rpc_port")]
    pub port: u16,

    /// Maximum concurrent requests
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_requests: usize,

    /// Request timeout in seconds
    #[serde(default = "default_request_timeout")]
    pub request_timeout_secs: u64,

    /// Enable session state storage for iterator operations
    #[serde(default)]
    pub session_enabled: bool,

    /// Maximum sessions
    #[serde(default = "default_max_sessions")]
    pub max_sessions: usize,
}

const fn default_rpc_enabled() -> bool {
    true
}

fn default_rpc_address() -> String {
    "127.0.0.1".to_string()
}

const fn default_rpc_port() -> u16 {
    10332
}

const fn default_max_concurrent() -> usize {
    100
}

const fn default_request_timeout() -> u64 {
    30
}

const fn default_max_sessions() -> usize {
    100
}

impl Default for RpcSettings {
    fn default() -> Self {
        Self {
            enabled: default_rpc_enabled(),
            address: default_rpc_address(),
            port: default_rpc_port(),
            max_concurrent_requests: default_max_concurrent(),
            request_timeout_secs: default_request_timeout(),
            session_enabled: false,
            max_sessions: default_max_sessions(),
        }
    }
}
