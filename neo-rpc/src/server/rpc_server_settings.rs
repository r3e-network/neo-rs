// Rust translation of Neo.Plugins.RpcServer.RpcServerSettings and
// RpcServersSettings. Provides JSON configuration deserialisation for the RPC
// server plugin.

use neo_error::CoreResult;
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
///
/// `Debug` is implemented manually to redact the credential fields
/// (`rpc_pass`, `ssl_cert_password`) so they never leak into logs or error
/// output.
#[derive(Clone, Deserialize, PartialEq, Eq)]
pub struct RpcServerConfig {
    /// Neo network magic served by this RPC endpoint.
    #[serde(default = "RpcServerConfig::default_network", alias = "Network")]
    pub network: u32,
    /// IP address the RPC listener binds to.
    #[serde(
        default = "RpcServerConfig::default_bind_address",
        alias = "BindAddress"
    )]
    pub bind_address: IpAddr,
    /// TCP port the RPC listener binds to.
    #[serde(default = "RpcServerConfig::default_port", alias = "Port")]
    pub port: u16,
    /// Path to the TLS certificate file, when TLS is configured externally.
    #[serde(default, alias = "SslCert")]
    pub ssl_cert: String,
    /// Password for the TLS certificate file.
    #[serde(default, alias = "SslCertPassword")]
    pub ssl_cert_password: String,
    /// Trusted client certificate authorities.
    #[serde(default, alias = "TrustedAuthorities")]
    pub trusted_authorities: Vec<String>,
    /// Maximum concurrently accepted RPC connections.
    #[serde(
        default = "RpcServerConfig::default_max_concurrent_connections",
        alias = "MaxConcurrentConnections"
    )]
    pub max_concurrent_connections: usize,
    /// Configured maximum requests per second for the in-process RPC limiter.
    ///
    /// The jsonrpsee dispatch path enforces this as a process-wide fallback because
    /// the current transport does not expose client IPs to method dispatch. Use an
    /// edge proxy for true per-client/IP rate limits on public deployments.
    #[serde(
        default = "RpcServerConfig::default_max_requests_per_second",
        alias = "MaxRequestsPerSecond"
    )]
    pub max_requests_per_second: u32,
    /// Configured burst capacity for the in-process rate limiter.
    #[serde(
        default = "RpcServerConfig::default_rate_limit_burst",
        alias = "RateLimitBurst"
    )]
    pub rate_limit_burst: u32,
    /// Maximum accepted JSON-RPC request body size, in bytes.
    #[serde(
        default = "RpcServerConfig::default_max_request_body_size",
        alias = "MaxRequestBodySize"
    )]
    pub max_request_body_size: usize,
    /// Optional RPC basic-auth username.
    #[serde(default, alias = "RpcUser")]
    pub rpc_user: String,
    /// Optional RPC basic-auth password.
    #[serde(default, alias = "RpcPass")]
    pub rpc_pass: String,
    /// Whether CORS headers are enabled.
    #[serde(default = "RpcServerConfig::default_enable_cors", alias = "EnableCors")]
    pub enable_cors: bool,
    /// Allowed CORS origins.
    #[serde(default, alias = "AllowOrigins")]
    pub allow_origins: Vec<String>,
    /// Idle keep-alive timeout in seconds; negative disables idle reaping.
    #[serde(
        default = "RpcServerConfig::default_keep_alive_timeout",
        alias = "KeepAliveTimeout"
    )]
    pub keep_alive_timeout: i32,
    /// Request header timeout in seconds.
    #[serde(
        default = "RpcServerConfig::default_request_headers_timeout",
        alias = "RequestHeadersTimeout"
    )]
    pub request_headers_timeout: u64,
    /// Maximum GAS allowed for an invoke call, in datoshi.
    #[serde(
        default = "RpcServerConfig::default_max_gas_invocation",
        deserialize_with = "deserialize_max_gas_invoke",
        alias = "MaxGasInvoke"
    )]
    pub max_gas_invoke: i64,
    /// Maximum wallet fee, in datoshi.
    #[serde(
        default = "RpcServerConfig::default_max_fee",
        deserialize_with = "deserialize_max_fee",
        alias = "MaxFee"
    )]
    pub max_fee: i64,
    /// Maximum iterator result items returned in one RPC response.
    #[serde(
        default = "RpcServerConfig::default_max_iterator_result_items",
        alias = "MaxIteratorResultItems"
    )]
    pub max_iterator_result_items: usize,
    /// Maximum VM stack items allowed in RPC invoke responses.
    #[serde(
        default = "RpcServerConfig::default_max_stack_size",
        alias = "MaxStackSize"
    )]
    pub max_stack_size: usize,
    /// RPC method names disabled for this endpoint.
    #[serde(default, alias = "DisabledMethods")]
    pub disabled_methods: Vec<String>,
    /// Whether invoke sessions are enabled.
    #[serde(default, alias = "SessionEnabled")]
    pub session_enabled: bool,
    /// Session expiration time in seconds.
    #[serde(
        default = "RpcServerConfig::default_session_expiration_seconds",
        alias = "SessionExpirationTime"
    )]
    pub session_expiration_time: u64,
    /// Page size used by `findstorage`.
    #[serde(
        default = "RpcServerConfig::default_find_storage_page_size",
        alias = "FindStoragePageSize"
    )]
    pub find_storage_page_size: usize,
    /// Maximum number of JSON-RPC calls allowed in a single batch request.
    /// Prevents amplification attacks where a single HTTP request bypasses
    /// per-method rate limiting. Matches C# `MaxBatchSize` (default 1024).
    #[serde(
        default = "RpcServerConfig::default_max_batch_size",
        alias = "MaxBatchSize"
    )]
    pub max_batch_size: usize,
}

impl std::fmt::Debug for RpcServerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RpcServerConfig")
            .field("network", &self.network)
            .field("bind_address", &self.bind_address)
            .field("port", &self.port)
            .field("ssl_cert", &self.ssl_cert)
            .field("ssl_cert_password", &"[redacted]")
            .field("trusted_authorities", &self.trusted_authorities)
            .field(
                "max_concurrent_connections",
                &self.max_concurrent_connections,
            )
            .field("max_requests_per_second", &self.max_requests_per_second)
            .field("rate_limit_burst", &self.rate_limit_burst)
            .field("max_request_body_size", &self.max_request_body_size)
            .field("rpc_user", &self.rpc_user)
            .field("rpc_pass", &"[redacted]")
            .field("enable_cors", &self.enable_cors)
            .field("allow_origins", &self.allow_origins)
            .field("keep_alive_timeout", &self.keep_alive_timeout)
            .field("request_headers_timeout", &self.request_headers_timeout)
            .field("max_gas_invoke", &self.max_gas_invoke)
            .field("max_fee", &self.max_fee)
            .field("max_iterator_result_items", &self.max_iterator_result_items)
            .field("max_stack_size", &self.max_stack_size)
            .field("disabled_methods", &self.disabled_methods)
            .field("session_enabled", &self.session_enabled)
            .field("session_expiration_time", &self.session_expiration_time)
            .field("find_storage_page_size", &self.find_storage_page_size)
            .field("max_batch_size", &self.max_batch_size)
            .finish()
    }
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
        100
    }

    const fn default_rate_limit_burst() -> u32 {
        200
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

    /// Return the request header timeout as a [`Duration`].
    #[must_use]
    pub const fn request_headers_timeout_duration(&self) -> Duration {
        Duration::from_secs(self.request_headers_timeout)
    }

    /// Return the keep-alive timeout as a [`Duration`], or `None` when disabled.
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

fn parse_gas_value(value: Value) -> CoreResult<i64> {
    match value {
        Value::Number(number) => parse_gas_number(&number),
        Value::String(text) => parse_gas_string(&text),
        Value::Null => Err(neo_error::CoreError::other("gas value cannot be null")),
        _ => Err(neo_error::CoreError::other(
            "gas value must be a number or string",
        )),
    }
}

fn parse_gas_number(number: &serde_json::Number) -> CoreResult<i64> {
    if let Some(int_value) = number.as_i64() {
        return apply_gas_threshold(int_value);
    }
    if let Some(uint_value) = number.as_u64() {
        let int_value = i64::try_from(uint_value)
            .map_err(|_| neo_error::CoreError::other("gas value exceeds i64"))?;
        return apply_gas_threshold(int_value);
    }
    let float_value = number
        .as_f64()
        .ok_or_else(|| neo_error::CoreError::other("gas value must be numeric"))?;
    convert_gas_units(float_value)
}

fn parse_gas_string(text: &str) -> CoreResult<i64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(neo_error::CoreError::other("gas value cannot be empty"));
    }
    if let Ok(int_value) = trimmed.parse::<i64>() {
        return apply_gas_threshold(int_value);
    }
    let float_value = trimmed
        .parse::<f64>()
        .map_err(|_| neo_error::CoreError::other("gas value must be numeric"))?;
    convert_gas_units(float_value)
}

fn apply_gas_threshold(value: i64) -> CoreResult<i64> {
    if value.abs() <= GAS_UNIT_THRESHOLD {
        value
            .checked_mul(RpcServerConfig::gas_datoshi_factor())
            .ok_or_else(|| neo_error::CoreError::other("gas value overflow"))
    } else {
        Ok(value)
    }
}

fn convert_gas_units(value: f64) -> CoreResult<i64> {
    if !value.is_finite() {
        return Err(neo_error::CoreError::other("gas value must be finite"));
    }
    let scaled = value * RpcServerConfig::gas_datoshi_factor() as f64;
    if scaled > i64::MAX as f64 || scaled < i64::MIN as f64 {
        return Err(neo_error::CoreError::other("gas value overflow"));
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
    /// Load process-wide RPC settings from a plugin configuration object.
    pub fn load(config: Option<&Value>) -> CoreResult<()> {
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

    fn validate(&self) -> CoreResult<()> {
        for server in &self.servers {
            let _has_auth = !server.rpc_user.trim().is_empty();

            if server.max_requests_per_second == 0 && !server.bind_address.is_loopback() {
                tracing::warn!(
                    target: "neo::rpc",
                    bind_address = %server.bind_address,
                    port = server.port,
                    "RPC rate limiting is disabled (MaxRequestsPerSecond=0) on a non-localhost \
                     bind address; this exposes the node to denial-of-service attacks"
                );
            }
        }
        Ok(())
    }

    /// Return a clone of the currently loaded RPC settings.
    pub fn current() -> Self {
        CURRENT_SETTINGS.read().clone()
    }

    /// Return configured RPC server endpoints.
    #[must_use]
    pub fn servers(&self) -> &[RpcServerConfig] {
        &self.servers
    }

    /// Return the unhandled-exception policy for RPC handlers.
    #[must_use]
    pub const fn exception_policy(&self) -> UnhandledExceptionPolicy {
        self.exception_policy
    }

    /// Return the server configuration matching `network`, if present.
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
#[path = "../tests/server/rpc_server_settings.rs"]
mod tests;
