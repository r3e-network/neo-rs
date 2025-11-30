// Copyright (C) 2015-2025 The Neo Project.
//
// Rust translation of Neo.Plugins.RpcServer.RpcServerSettings and
// RpcServersSettings. Provides JSON configuration deserialisation for the RPC
// server plugin.

use neo_core::plugins::UnhandledExceptionPolicy;
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use serde::Deserialize;
use serde_json::Value;
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

/// Represents a single RPC server configuration block (`RpcServersSettings`).
#[derive(Debug, Clone, Deserialize)]
pub struct RpcServerConfig {
    #[serde(default = "RpcServerConfig::default_network")]
    pub network: u32,
    #[serde(default = "RpcServerConfig::default_bind_address")]
    pub bind_address: IpAddr,
    #[serde(default = "RpcServerConfig::default_port")]
    pub port: u16,
    #[serde(default)]
    pub ssl_cert: String,
    #[serde(default)]
    pub ssl_cert_password: String,
    #[serde(default)]
    pub trusted_authorities: Vec<String>,
    #[serde(default = "RpcServerConfig::default_max_concurrent_connections")]
    pub max_concurrent_connections: usize,
    #[serde(default = "RpcServerConfig::default_max_request_body_size")]
    pub max_request_body_size: usize,
    #[serde(default)]
    pub rpc_user: String,
    #[serde(default)]
    pub rpc_pass: String,
    #[serde(default = "RpcServerConfig::default_enable_cors")]
    pub enable_cors: bool,
    #[serde(default)]
    pub allow_origins: Vec<String>,
    #[serde(default = "RpcServerConfig::default_keep_alive_timeout")]
    pub keep_alive_timeout: i32,
    #[serde(default = "RpcServerConfig::default_request_headers_timeout")]
    pub request_headers_timeout: u64,
    #[serde(default = "RpcServerConfig::default_max_gas_invocation")]
    pub max_gas_invoke: i64,
    #[serde(default = "RpcServerConfig::default_max_fee")]
    pub max_fee: i64,
    #[serde(default = "RpcServerConfig::default_max_iterator_result_items")]
    pub max_iterator_result_items: usize,
    #[serde(default = "RpcServerConfig::default_max_stack_size")]
    pub max_stack_size: usize,
    #[serde(default)]
    pub disabled_methods: Vec<String>,
    #[serde(default)]
    pub session_enabled: bool,
    #[serde(default = "RpcServerConfig::default_session_expiration_seconds")]
    pub session_expiration_time: u64,
    #[serde(default = "RpcServerConfig::default_find_storage_page_size")]
    pub find_storage_page_size: usize,
}

impl RpcServerConfig {
    const fn default_network() -> u32 {
        5_195_086
    }

    fn default_bind_address() -> IpAddr {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    }

    const fn default_port() -> u16 {
        10332
    }

    const fn default_max_concurrent_connections() -> usize {
        40
    }

    const fn default_max_request_body_size() -> usize {
        5 * 1024 * 1024
    }

    const fn default_enable_cors() -> bool {
        true
    }

    const fn default_keep_alive_timeout() -> i32 {
        60
    }

    const fn default_request_headers_timeout() -> u64 {
        15
    }

    const fn gas_datoshi_factor() -> i64 {
        100_000_000
    }

    const fn default_max_gas_invocation() -> i64 {
        10 * Self::gas_datoshi_factor()
    }

    const fn default_max_fee() -> i64 {
        (Self::gas_datoshi_factor() as f64 * 0.1) as i64
    }

    const fn default_max_iterator_result_items() -> usize {
        100
    }

    const fn default_max_stack_size() -> usize {
        u16::MAX as usize
    }

    const fn default_session_expiration_seconds() -> u64 {
        60
    }

    const fn default_find_storage_page_size() -> usize {
        50
    }

    pub fn request_headers_timeout_duration(&self) -> Duration {
        Duration::from_secs(self.request_headers_timeout)
    }

    pub fn keep_alive_timeout_duration(&self) -> Option<Duration> {
        if self.keep_alive_timeout < 0 {
            None
        } else {
            Some(Duration::from_secs(self.keep_alive_timeout as u64))
        }
    }
}

/// Global RPC server settings (mirrors `RpcServerSettings` in C#).
#[derive(Debug, Clone)]
pub struct RpcServerSettings {
    servers: Vec<RpcServerConfig>,
    exception_policy: UnhandledExceptionPolicy,
}

static CURRENT_SETTINGS: Lazy<RwLock<RpcServerSettings>> = Lazy::new(|| {
    RwLock::new(RpcServerSettings {
        servers: vec![RpcServerConfig::default()],
        exception_policy: UnhandledExceptionPolicy::Ignore,
    })
});

impl Default for RpcServerConfig {
    fn default() -> Self {
        Self {
            network: Self::default_network(),
            bind_address: Self::default_bind_address(),
            port: Self::default_port(),
            ssl_cert: String::new(),
            ssl_cert_password: String::new(),
            trusted_authorities: Vec::new(),
            max_concurrent_connections: Self::default_max_concurrent_connections(),
            max_request_body_size: Self::default_max_request_body_size(),
            rpc_user: String::new(),
            rpc_pass: String::new(),
            enable_cors: Self::default_enable_cors(),
            allow_origins: Vec::new(),
            keep_alive_timeout: Self::default_keep_alive_timeout(),
            request_headers_timeout: Self::default_request_headers_timeout(),
            max_gas_invoke: Self::default_max_gas_invocation(),
            max_fee: Self::default_max_fee(),
            max_iterator_result_items: Self::default_max_iterator_result_items(),
            max_stack_size: Self::default_max_stack_size(),
            disabled_methods: Vec::new(),
            session_enabled: false,
            session_expiration_time: Self::default_session_expiration_seconds(),
            find_storage_page_size: Self::default_find_storage_page_size(),
        }
    }
}

impl RpcServerSettings {
    pub fn load(config: Option<&Value>) {
        let mut guard = CURRENT_SETTINGS.write();
        if let Some(value) = config {
            let plugin_configuration = value.get("PluginConfiguration").unwrap_or(value);
            let servers_section = plugin_configuration
                .get("Servers")
                .cloned()
                .unwrap_or(Value::Null);
            let servers: Vec<RpcServerConfig> = serde_json::from_value(servers_section)
                .unwrap_or_else(|_| vec![RpcServerConfig::default()]);
            let exception_policy = plugin_configuration
                .get("UnhandledExceptionPolicy")
                .and_then(|policy| serde_json::from_value(policy.clone()).ok())
                .unwrap_or_default();
            guard.servers = if servers.is_empty() {
                vec![RpcServerConfig::default()]
            } else {
                servers
            };
            guard.exception_policy = exception_policy;
        } else {
            *guard = RpcServerSettings::default();
        }
    }

    pub fn current() -> RpcServerSettings {
        CURRENT_SETTINGS.read().clone()
    }

    pub fn servers(&self) -> &[RpcServerConfig] {
        &self.servers
    }

    pub fn exception_policy(&self) -> UnhandledExceptionPolicy {
        self.exception_policy
    }

    pub fn server_for_network(&self, network: u32) -> Option<RpcServerConfig> {
        self.servers
            .iter()
            .find(|cfg| cfg.network == network)
            .cloned()
    }
}

impl Default for RpcServerSettings {
    fn default() -> Self {
        Self {
            servers: vec![RpcServerConfig::default()],
            exception_policy: UnhandledExceptionPolicy::Ignore,
        }
    }
}
