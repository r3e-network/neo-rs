use std::collections::HashMap;

use serde::Deserialize;

/// `[observability]`: external error reporting and heartbeat monitoring.
#[derive(Debug, Clone, Deserialize)]
pub(in crate::node) struct ObservabilitySection {
    /// Whether to install node observability hooks and background tasks.
    #[serde(default, alias = "Enabled")]
    pub(in crate::node) enabled: bool,
    /// Service name sent to external providers.
    #[serde(default, alias = "ServiceName")]
    pub(in crate::node) service_name: Option<String>,
    /// Deployment environment (`production`, `testnet`, `local`, ...).
    #[serde(default, alias = "Environment")]
    pub(in crate::node) environment: Option<String>,
    /// Operator-provided stable node identifier.
    #[serde(default, alias = "NodeId")]
    pub(in crate::node) node_id: Option<String>,
    /// Whether to report Rust panics through the configured error endpoints.
    #[serde(default = "super::default_true", alias = "CapturePanics")]
    pub(in crate::node) capture_panics: bool,
    /// HTTP timeout for outbound observability requests.
    #[serde(
        default = "default_observability_request_timeout_ms",
        alias = "RequestTimeoutMs"
    )]
    pub(in crate::node) request_timeout_ms: u64,
    /// Total attempts per error report before giving up (>=1). A transient
    /// network failure should not silently drop a crash/panic report, so the
    /// reporter retries with exponential backoff up to this many attempts.
    #[serde(
        default = "default_observability_max_send_attempts",
        alias = "MaxSendAttempts"
    )]
    pub(in crate::node) max_send_attempts: u32,
    /// Base backoff between error-report retries, in milliseconds. The delay
    /// doubles each attempt (capped) so the first retry waits this long.
    #[serde(
        default = "default_observability_retry_backoff_ms",
        alias = "RetryBackoffMs"
    )]
    pub(in crate::node) retry_backoff_ms: u64,
    /// Default heartbeat cadence when an endpoint does not override it.
    #[serde(
        default = "default_observability_heartbeat_interval_seconds",
        alias = "HeartbeatIntervalSeconds"
    )]
    pub(in crate::node) heartbeat_interval_seconds: u64,
    /// Error reporting destinations.
    #[serde(default, alias = "ErrorEndpoints")]
    pub(in crate::node) error_endpoints: Vec<ObservabilityErrorEndpoint>,
    /// Heartbeat destinations such as Better Stack heartbeat URLs.
    #[serde(default, alias = "HeartbeatEndpoints")]
    pub(in crate::node) heartbeat_endpoints: Vec<ObservabilityHeartbeatEndpoint>,
}

impl Default for ObservabilitySection {
    fn default() -> Self {
        Self {
            enabled: false,
            service_name: None,
            environment: None,
            node_id: None,
            capture_panics: true,
            request_timeout_ms: default_observability_request_timeout_ms(),
            max_send_attempts: default_observability_max_send_attempts(),
            retry_backoff_ms: default_observability_retry_backoff_ms(),
            heartbeat_interval_seconds: default_observability_heartbeat_interval_seconds(),
            error_endpoints: Vec::new(),
            heartbeat_endpoints: Vec::new(),
        }
    }
}

/// One outbound error reporting destination.
#[derive(Debug, Clone, Deserialize)]
pub(in crate::node) struct ObservabilityErrorEndpoint {
    /// Whether this destination is active.
    #[serde(default = "super::default_true", alias = "Enabled")]
    pub(in crate::node) enabled: bool,
    /// `custom_json`, `better_stack_logs`, or `google_error_reporting`.
    #[serde(default, alias = "Kind")]
    pub(in crate::node) kind: Option<String>,
    /// Human-readable destination name used in logs.
    #[serde(default, alias = "Name")]
    pub(in crate::node) name: Option<String>,
    /// Destination URL. Optional for Google when `project_id` is set.
    #[serde(default, alias = "Url")]
    pub(in crate::node) url: Option<String>,
    /// Inline bearer token. Prefer `token_env` for production configs.
    #[serde(default, alias = "Token")]
    pub(in crate::node) token: Option<String>,
    /// Environment variable holding the bearer token.
    #[serde(default, alias = "TokenEnv")]
    pub(in crate::node) token_env: Option<String>,
    /// Google Cloud project id for `google_error_reporting`.
    #[serde(default, alias = "ProjectId")]
    pub(in crate::node) project_id: Option<String>,
    /// Extra HTTP headers for custom providers.
    #[serde(default, alias = "Headers")]
    pub(in crate::node) headers: HashMap<String, String>,
    /// Extra HTTP headers whose values are read from environment variables.
    #[serde(default, alias = "HeadersEnv")]
    pub(in crate::node) headers_env: HashMap<String, String>,
}

impl Default for ObservabilityErrorEndpoint {
    fn default() -> Self {
        Self {
            enabled: true,
            kind: None,
            name: None,
            url: None,
            token: None,
            token_env: None,
            project_id: None,
            headers: HashMap::new(),
            headers_env: HashMap::new(),
        }
    }
}

/// One outbound heartbeat destination.
#[derive(Debug, Clone, Deserialize)]
pub(in crate::node) struct ObservabilityHeartbeatEndpoint {
    /// Whether this heartbeat is active.
    #[serde(default = "super::default_true", alias = "Enabled")]
    pub(in crate::node) enabled: bool,
    /// Human-readable destination name used in logs.
    #[serde(default, alias = "Name")]
    pub(in crate::node) name: Option<String>,
    /// Heartbeat URL to call.
    #[serde(default, alias = "Url")]
    pub(in crate::node) url: Option<String>,
    /// HTTP method; defaults to `GET`.
    #[serde(default, alias = "Method")]
    pub(in crate::node) method: Option<String>,
    /// Optional per-destination heartbeat cadence.
    #[serde(default, alias = "IntervalSeconds")]
    pub(in crate::node) interval_seconds: Option<u64>,
    /// Inline bearer token. Prefer `token_env` for production configs.
    #[serde(default, alias = "Token")]
    pub(in crate::node) token: Option<String>,
    /// Environment variable holding the bearer token.
    #[serde(default, alias = "TokenEnv")]
    pub(in crate::node) token_env: Option<String>,
    /// Extra HTTP headers for custom providers.
    #[serde(default, alias = "Headers")]
    pub(in crate::node) headers: HashMap<String, String>,
    /// Extra HTTP headers whose values are read from environment variables.
    #[serde(default, alias = "HeadersEnv")]
    pub(in crate::node) headers_env: HashMap<String, String>,
}

impl Default for ObservabilityHeartbeatEndpoint {
    fn default() -> Self {
        Self {
            enabled: true,
            name: None,
            url: None,
            method: None,
            interval_seconds: None,
            token: None,
            token_env: None,
            headers: HashMap::new(),
            headers_env: HashMap::new(),
        }
    }
}

const fn default_observability_request_timeout_ms() -> u64 {
    5_000
}

const fn default_observability_max_send_attempts() -> u32 {
    3
}

const fn default_observability_retry_backoff_ms() -> u64 {
    250
}

const fn default_observability_heartbeat_interval_seconds() -> u64 {
    60
}
