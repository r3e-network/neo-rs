//! Oracle service settings (matches Neo.Plugins.OracleService configuration).

use crate::unhandled_exception_policy::UnhandledExceptionPolicy;
use std::time::Duration;

/// Maximum response size for oracle requests (64KB).
pub const MAX_ORACLE_RESPONSE_SIZE: usize = 64 * 1024;

/// Default request timeout for oracle HTTPS requests (30 seconds).
pub const DEFAULT_ORACLE_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Oracle service configuration settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OracleServiceSettings {
    pub network: u32,
    pub nodes: Vec<String>,
    pub max_task_timeout: Duration,
    pub max_oracle_timeout: Duration,
    pub allow_private_host: bool,
    pub allowed_content_types: Vec<String>,
    pub https_timeout: Duration,
    pub neofs_endpoint: String,
    pub neofs_timeout: Duration,
    pub neofs_bearer_token: Option<String>,
    pub neofs_bearer_signature: Option<String>,
    pub neofs_bearer_signature_key: Option<String>,
    pub neofs_wallet_connect: bool,
    pub neofs_auto_sign_bearer: bool,
    pub neofs_use_grpc: bool,
    pub auto_start: bool,
    pub exception_policy: UnhandledExceptionPolicy,
    /// URL whitelist - only these URLs/patterns are allowed (empty = allow all non-blocked).
    pub url_whitelist: Vec<String>,
    /// URL blacklist - these URLs/patterns are blocked.
    pub url_blacklist: Vec<String>,
    /// Maximum response size in bytes (default: 64KB).
    pub max_response_size: usize,
    /// Enable request deduplication.
    pub enable_deduplication: bool,
}

impl Default for OracleServiceSettings {
    fn default() -> Self {
        Self {
            network: 860_833_102,
            nodes: Vec::new(),
            max_task_timeout: Duration::from_millis(432_000_000),
            max_oracle_timeout: Duration::from_millis(15_000),
            allow_private_host: false,
            allowed_content_types: vec!["application/json".to_string()],
            https_timeout: DEFAULT_ORACLE_REQUEST_TIMEOUT,
            neofs_endpoint: "http://127.0.0.1:8080".to_string(),
            neofs_timeout: Duration::from_millis(15_000),
            neofs_bearer_token: None,
            neofs_bearer_signature: None,
            neofs_bearer_signature_key: None,
            neofs_wallet_connect: false,
            neofs_auto_sign_bearer: false,
            neofs_use_grpc: cfg!(feature = "neofs-grpc"),
            auto_start: false,
            exception_policy: UnhandledExceptionPolicy::Ignore,
            url_whitelist: Vec::new(),
            url_blacklist: vec![
                "localhost".to_string(),
                "127.0.0.1".to_string(),
                "::1".to_string(),
                "0.0.0.0".to_string(),
            ],
            max_response_size: MAX_ORACLE_RESPONSE_SIZE,
            enable_deduplication: true,
        }
    }
}

impl OracleServiceSettings {
    /// Returns true if a content type is allowed.
    pub fn is_content_type_allowed(&self, content_type: &str) -> bool {
        self.allowed_content_types
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(content_type))
    }

    /// Validates a URL against whitelist and blacklist.
    /// Returns true if the URL is allowed.
    pub fn is_url_allowed(&self, url: &str) -> bool {
        // Check blacklist first
        for blocked in &self.url_blacklist {
            if url.contains(blocked) {
                return false;
            }
        }

        // If whitelist is not empty, URL must match at least one pattern
        if !self.url_whitelist.is_empty() {
            return self
                .url_whitelist
                .iter()
                .any(|allowed| url.contains(allowed));
        }

        true
    }

    /// Validates that a response size is within limits.
    pub fn is_response_size_allowed(&self, size: usize) -> bool {
        size <= self.max_response_size
    }

    /// Ensures allowed content types are initialized with defaults.
    pub fn normalize(&mut self) {
        if self.allowed_content_types.is_empty() {
            self.allowed_content_types
                .push("application/json".to_string());
        }
        if self
            .neofs_bearer_token
            .as_ref()
            .map(|value| value.trim().is_empty())
            .unwrap_or(false)
        {
            self.neofs_bearer_token = None;
        }
        if self
            .neofs_bearer_signature
            .as_ref()
            .map(|value| value.trim().is_empty())
            .unwrap_or(false)
        {
            self.neofs_bearer_signature = None;
        }
        if self
            .neofs_bearer_signature_key
            .as_ref()
            .map(|value| value.trim().is_empty())
            .unwrap_or(false)
        {
            self.neofs_bearer_signature_key = None;
        }
        // Ensure max_response_size has a reasonable minimum
        if self.max_response_size == 0 {
            self.max_response_size = MAX_ORACLE_RESPONSE_SIZE;
        }
        // Ensure timeout is at least 1 second
        if self.https_timeout < Duration::from_secs(1) {
            self.https_timeout = DEFAULT_ORACLE_REQUEST_TIMEOUT;
        }
    }
}
