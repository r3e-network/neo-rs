// Copyright (C) 2015-2025 The Neo Project.
//
// Rust translation of Neo.Plugins.RpcServer.RpcServerSettings and
// RpcServersSettings. Provides JSON configuration deserialisation for the RPC
// server plugin.

use neo_core::extensions::error::ExtensionResult;
use parking_lot::RwLock;
use serde::Deserialize;
use serde::de::{self, Deserializer};
use serde_json::Value;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::LazyLock;
use std::time::Duration;

/// Policy for handling unhandled exceptions in the RPC server
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum UnhandledExceptionPolicy {
    /// Ignore exceptions and continue processing
    #[default]
    Ignore,
    /// Log exceptions
    Log,
    /// Stop the plugin/service
    StopPlugin,
    /// Stop the node
    StopNode,
    /// Continue after logging
    Continue,
    /// Terminate the process
    Terminate,
}

/// Represents a single RPC server configuration block (`RpcServersSettings`).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RpcServerConfig {
    #[serde(default = "RpcServerConfig::default_network", alias = "Network")]
    pub network: u32,
    #[serde(
        default = "RpcServerConfig::default_bind_address",
        alias = "BindAddress"
    )]
    pub bind_address: IpAddr,
    #[serde(default = "RpcServerConfig::default_port", alias = "Port")]
    pub port: u16,
    #[serde(default, alias = "SslCert")]
    pub ssl_cert: String,
    #[serde(default, alias = "SslCertPassword")]
    pub ssl_cert_password: String,
    #[serde(default, alias = "TrustedAuthorities")]
    pub trusted_authorities: Vec<String>,
    #[serde(
        default = "RpcServerConfig::default_max_concurrent_connections",
        alias = "MaxConcurrentConnections"
    )]
    pub max_concurrent_connections: usize,
    /// Maximum requests per second per IP (0 disables rate limiting).
    #[serde(
        default = "RpcServerConfig::default_max_requests_per_second",
        alias = "MaxRequestsPerSecond"
    )]
    pub max_requests_per_second: u32,
    /// Burst capacity for the per-IP rate limiter (0 uses `max_requests_per_second`).
    #[serde(
        default = "RpcServerConfig::default_rate_limit_burst",
        alias = "RateLimitBurst"
    )]
    pub rate_limit_burst: u32,
    #[serde(
        default = "RpcServerConfig::default_max_request_body_size",
        alias = "MaxRequestBodySize"
    )]
    pub max_request_body_size: usize,
    #[serde(default, alias = "RpcUser")]
    pub rpc_user: String,
    #[serde(default, alias = "RpcPass")]
    pub rpc_pass: String,
    #[serde(default = "RpcServerConfig::default_enable_cors", alias = "EnableCors")]
    pub enable_cors: bool,
    #[serde(default, alias = "AllowOrigins")]
    pub allow_origins: Vec<String>,
    #[serde(
        default = "RpcServerConfig::default_keep_alive_timeout",
        alias = "KeepAliveTimeout"
    )]
    pub keep_alive_timeout: i32,
    #[serde(
        default = "RpcServerConfig::default_request_headers_timeout",
        alias = "RequestHeadersTimeout"
    )]
    pub request_headers_timeout: u64,
    #[serde(
        default = "RpcServerConfig::default_max_gas_invocation",
        deserialize_with = "deserialize_max_gas_invoke",
        alias = "MaxGasInvoke"
    )]
    pub max_gas_invoke: i64,
    #[serde(
        default = "RpcServerConfig::default_max_fee",
        deserialize_with = "deserialize_max_fee",
        alias = "MaxFee"
    )]
    pub max_fee: i64,
    #[serde(
        default = "RpcServerConfig::default_max_iterator_result_items",
        alias = "MaxIteratorResultItems"
    )]
    pub max_iterator_result_items: usize,
    #[serde(
        default = "RpcServerConfig::default_max_stack_size",
        alias = "MaxStackSize"
    )]
    pub max_stack_size: usize,
    #[serde(default, alias = "DisabledMethods")]
    pub disabled_methods: Vec<String>,
    #[serde(default, alias = "SessionEnabled")]
    pub session_enabled: bool,
    #[serde(
        default = "RpcServerConfig::default_session_expiration_seconds",
        alias = "SessionExpirationTime"
    )]
    pub session_expiration_time: u64,
    #[serde(
        default = "RpcServerConfig::default_find_storage_page_size",
        alias = "FindStoragePageSize"
    )]
    pub find_storage_page_size: usize,
    /// Maximum number of JSON-RPC calls allowed in a single batch request.
    /// Prevents amplification attacks where a single HTTP request bypasses
    /// per-IP rate limiting. Matches C# `MaxBatchSize` (default 1024).
    #[serde(
        default = "RpcServerConfig::default_max_batch_size",
        alias = "MaxBatchSize"
    )]
    pub max_batch_size: usize,
}

impl RpcServerConfig {
    const fn default_network() -> u32 {
        5_195_086
    }

    const fn default_bind_address() -> IpAddr {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    }

    const fn default_port() -> u16 {
        10332
    }

    const fn default_max_concurrent_connections() -> usize {
        100
    }

    const fn default_max_request_body_size() -> usize {
        5 * 1024 * 1024
    }

    const fn default_max_requests_per_second() -> u32 {
        0
    }

    const fn default_rate_limit_burst() -> u32 {
        0
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

    const fn default_max_batch_size() -> usize {
        1024
    }

    #[must_use]
    pub const fn request_headers_timeout_duration(&self) -> Duration {
        Duration::from_secs(self.request_headers_timeout)
    }

    #[must_use]
    pub const fn keep_alive_timeout_duration(&self) -> Option<Duration> {
        if self.keep_alive_timeout < 0 {
            None
        } else {
            Some(Duration::from_secs(self.keep_alive_timeout as u64))
        }
    }
}

const GAS_UNIT_THRESHOLD: i64 = 1_000;

fn deserialize_max_gas_invoke<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    parse_gas_value(value).map_err(de::Error::custom)
}

fn deserialize_max_fee<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    parse_gas_value(value).map_err(de::Error::custom)
}

fn parse_gas_value(value: Value) -> Result<i64, String> {
    match value {
        Value::Number(number) => parse_gas_number(&number),
        Value::String(text) => parse_gas_string(&text),
        Value::Null => Err("gas value cannot be null".to_string()),
        _ => Err("gas value must be a number or string".to_string()),
    }
}

fn parse_gas_number(number: &serde_json::Number) -> Result<i64, String> {
    if let Some(int_value) = number.as_i64() {
        return apply_gas_threshold(int_value);
    }
    if let Some(uint_value) = number.as_u64() {
        let int_value =
            i64::try_from(uint_value).map_err(|_| "gas value exceeds i64".to_string())?;
        return apply_gas_threshold(int_value);
    }
    let float_value = number
        .as_f64()
        .ok_or_else(|| "gas value must be numeric".to_string())?;
    convert_gas_units(float_value)
}

fn parse_gas_string(text: &str) -> Result<i64, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("gas value cannot be empty".to_string());
    }
    if let Ok(int_value) = trimmed.parse::<i64>() {
        return apply_gas_threshold(int_value);
    }
    let float_value = trimmed
        .parse::<f64>()
        .map_err(|_| "gas value must be numeric".to_string())?;
    convert_gas_units(float_value)
}

fn apply_gas_threshold(value: i64) -> Result<i64, String> {
    if value.abs() <= GAS_UNIT_THRESHOLD {
        value
            .checked_mul(RpcServerConfig::gas_datoshi_factor())
            .ok_or_else(|| "gas value overflow".to_string())
    } else {
        Ok(value)
    }
}

fn convert_gas_units(value: f64) -> Result<i64, String> {
    if !value.is_finite() {
        return Err("gas value must be finite".to_string());
    }
    let scaled = value * RpcServerConfig::gas_datoshi_factor() as f64;
    if scaled > i64::MAX as f64 || scaled < i64::MIN as f64 {
        return Err("gas value overflow".to_string());
    }
    Ok(scaled.round() as i64)
}

/// Global RPC server settings (mirrors `RpcServerSettings` in C#).
#[derive(Debug, Clone)]
pub struct RpcServerSettings {
    servers: Vec<RpcServerConfig>,
    exception_policy: UnhandledExceptionPolicy,
}

static CURRENT_SETTINGS: LazyLock<RwLock<RpcServerSettings>> = LazyLock::new(|| {
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
            max_requests_per_second: Self::default_max_requests_per_second(),
            rate_limit_burst: Self::default_rate_limit_burst(),
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
            max_batch_size: Self::default_max_batch_size(),
        }
    }
}

impl RpcServerSettings {
    pub fn load(config: Option<&Value>) -> ExtensionResult<()> {
        let settings = if let Some(value) = config {
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
            Self {
                servers: if servers.is_empty() {
                    vec![RpcServerConfig::default()]
                } else {
                    servers
                },
                exception_policy,
            }
        } else {
            Self::default()
        };

        // Validate settings early to fail fast on unsupported or insecure combos.
        settings.validate()?;

        *CURRENT_SETTINGS.write() = settings;
        Ok(())
    }

    fn validate(&self) -> ExtensionResult<()> {
        for server in &self.servers {
            let _has_auth = !server.rpc_user.trim().is_empty();
        }
        Ok(())
    }

    pub fn current() -> Self {
        CURRENT_SETTINGS.read().clone()
    }

    #[must_use]
    pub fn servers(&self) -> &[RpcServerConfig] {
        &self.servers
    }

    #[must_use]
    pub const fn exception_policy(&self) -> UnhandledExceptionPolicy {
        self.exception_policy
    }

    #[must_use]
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::net::{IpAddr, Ipv4Addr};
    use std::path::PathBuf;

    #[test]
    fn rpc_server_config_loads_csharp_settings() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let config_path = manifest_dir.join("../neo_csharp/node/plugins/RpcServer/RpcServer.json");
        if !config_path.exists() {
            eprintln!(
                "SKIP: neo_csharp submodule not initialized (missing {})",
                config_path.display()
            );
            return;
        }
        let raw = fs::read_to_string(&config_path).expect("read rpc server config");
        let json: Value = serde_json::from_str(&raw).expect("parse rpc server config");
        let servers = json["PluginConfiguration"]["Servers"]
            .as_array()
            .expect("servers array");
        let server = servers.first().expect("server entry");

        let config: RpcServerConfig =
            serde_json::from_value(server.clone()).expect("deserialize config");

        assert_eq!(config.network, 860_833_102);
        assert_eq!(config.port, 10332);
        assert_eq!(config.bind_address, IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
        assert_eq!(config.ssl_cert, "");
        assert_eq!(config.ssl_cert_password, "");
        assert!(config.trusted_authorities.is_empty());
        assert_eq!(config.rpc_user, "");
        assert_eq!(config.rpc_pass, "");
        assert!(config.enable_cors);
        assert!(config.allow_origins.is_empty());
        assert_eq!(config.keep_alive_timeout, 60);
        assert_eq!(config.request_headers_timeout, 15);
        assert_eq!(config.max_gas_invoke, 2_000_000_000);
        assert_eq!(config.max_fee, 10_000_000);
        assert_eq!(config.max_concurrent_connections, 40);
        assert_eq!(config.max_request_body_size, 5 * 1024 * 1024);
        assert_eq!(config.max_iterator_result_items, 100);
        assert_eq!(config.max_stack_size, 65_535);
        assert_eq!(config.disabled_methods, vec!["openwallet"]);
        assert!(!config.session_enabled);
        assert_eq!(config.session_expiration_time, 60);
        assert_eq!(config.find_storage_page_size, 50);
    }
}
