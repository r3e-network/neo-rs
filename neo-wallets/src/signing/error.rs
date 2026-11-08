//! Structured errors returned by the unified signing service.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// Current wire schema version for signing errors.
pub const SIGN_ERROR_SCHEMA_VERSION: u16 = 1;

/// Stable machine-readable signing error categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignErrorCode {
    /// Authentication information is missing or invalid.
    Unauthenticated,
    /// The active policy rejected the request.
    PolicyDenied,
    /// The request is malformed.
    InvalidRequest,
    /// The requested key or key version is unavailable.
    KeyNotFound,
    /// The requested key is disabled.
    KeyDisabled,
    /// The selected provider lacks an explicitly required capability.
    CapabilityUnavailable,
    /// The request deadline elapsed before provider execution began.
    DeadlineExceeded,
    /// Provider execution timed out after dispatch; completion is unknown.
    ResultUnknown,
    /// An idempotency key or request ID was reused for different content.
    IdempotencyConflict,
    /// A matching request is still being processed.
    RequestInProgress,
    /// A replay nonce was already consumed.
    ReplayDetected,
    /// A fee or quota limit was exceeded.
    LimitExceeded,
    /// The signing provider failed.
    ProviderFailure,
    /// Provider output failed independent cryptographic verification.
    VerificationFailed,
    /// Persistent state could not be read or updated.
    StorageFailure,
    /// A response stored for idempotency was invalid.
    CorruptState,
}

/// Versioned, structured error suitable for service boundaries and audit logs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignError {
    /// Error schema version.
    pub schema_version: u16,
    /// Stable error category.
    pub code: SignErrorCode,
    /// Human-readable diagnostic message.
    pub message: String,
    /// Whether retrying the same operation may succeed.
    pub retryable: bool,
    /// Request ID, when one was available.
    pub request_id: Option<String>,
    /// Additional non-secret structured details.
    pub details: BTreeMap<String, String>,
}

impl SignError {
    /// Creates a structured signing error.
    pub fn new(code: SignErrorCode, message: impl Into<String>) -> Self {
        Self {
            schema_version: SIGN_ERROR_SCHEMA_VERSION,
            code,
            message: message.into(),
            retryable: false,
            request_id: None,
            details: BTreeMap::new(),
        }
    }

    /// Attaches a request ID.
    #[must_use]
    pub fn for_request(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }

    /// Marks whether the error is retryable.
    #[must_use]
    pub fn retryable(mut self, value: bool) -> Self {
        self.retryable = value;
        self
    }

    /// Adds a structured detail.
    #[must_use]
    pub fn with_detail(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.details.insert(key.into(), value.into());
        self
    }
}

impl fmt::Display for SignError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl std::error::Error for SignError {}

/// Result type used by signing APIs.
pub type SignResult<T> = Result<T, SignError>;
