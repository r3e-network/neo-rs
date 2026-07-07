//! # neo-rpc::server::rpc_server_settings
//!
//! RPC server settings and configuration records.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `config`: `RpcServerConfig` defaults and redacted debug formatting.
//! - `gas`: C#-compatible GAS setting deserializers.
//! - `registry`: Process-wide settings loading, validation, and lookup.
//! - `tests`: Module-local tests and regression coverage.

// Rust translation of Neo.Plugins.RpcServer.RpcServerSettings and
// RpcServersSettings. Provides JSON configuration deserialisation for the RPC
// server plugin.

mod config;
mod gas;
mod registry;

use gas::{deserialize_max_fee, deserialize_max_gas_invoke};
pub use registry::RpcServerSettings;
use serde::Deserialize;
use std::net::{IpAddr, Ipv4Addr};
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

#[cfg(test)]
#[path = "../../tests/server/handlers/rpc_server_settings.rs"]
mod tests;
