//! Oracle service settings (matches Neo.Plugins.OracleService configuration).

use crate::unhandled_exception_policy::UnhandledExceptionPolicy;
use std::time::Duration;

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
            https_timeout: Duration::from_millis(5_000),
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
    }
}
