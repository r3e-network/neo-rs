//! Error types for the Neo Core crate
//!
//! This module provides comprehensive error handling for core Neo operations,
//! including type conversions, serialization, and system-level errors.

use neo_primitives::PrimitiveError;
use thiserror::Error;

/// Core module errors
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum CoreError {
    /// Invalid format error with detailed description
    #[error("Invalid format: {message}")]
    InvalidFormat {
        /// Error message describing the format issue
        message: String,
    },

    /// Invalid data error with context
    #[error("Invalid data: {message}")]
    InvalidData {
        /// Error message describing the data issue
        message: String,
    },

    /// I/O operation failed
    #[error("I/O error: {message}")]
    Io {
        /// Error message describing the I/O issue
        message: String,
    },

    /// Serialization failed
    #[error("Serialization error: {message}")]
    Serialization {
        /// Error message describing the serialization issue
        message: String,
    },

    /// Deserialization failed
    #[error("Deserialization error: {message}")]
    Deserialization {
        /// Error message describing the deserialization issue
        message: String,
    },

    /// Invalid operation attempted
    #[error("Invalid operation: {message}")]
    InvalidOperation {
        /// Error message describing the invalid operation
        message: String,
    },

    /// System-level error
    #[error("System error: {message}")]
    System {
        /// Error message describing the system issue
        message: String,
    },

    /// Insufficient gas for operation
    #[error("Insufficient gas: required {required}, available {available}")]
    InsufficientGas {
        /// Amount of gas required for the operation
        required: u64,
        /// Amount of gas available
        available: u64,
    },

    /// Cryptographic operation failed
    #[error("Cryptographic error: {message}")]
    Cryptographic {
        /// Error message describing the cryptographic issue
        message: String,
    },

    /// Base58 decoding failed
    #[error("Base58 decoding error: {message}")]
    Base58Decode {
        /// Error message describing the decoding issue
        message: String,
    },

    /// Invalid Wallet Import Format string
    #[error("Invalid WIF format")]
    InvalidWif,

    /// Invalid private key bytes
    #[error("Invalid private key")]
    InvalidPrivateKey,

    /// Scrypt key derivation failed
    #[error("Scrypt error: {message}")]
    Scrypt {
        /// Error message describing the scrypt failure
        message: String,
    },

    /// AES encryption/decryption failed
    #[error("AES error: {message}")]
    Aes {
        /// Error message describing the AES failure
        message: String,
    },

    /// Invalid NEP-2 payload
    #[error("Invalid NEP-2 key")]
    InvalidNep2Key,

    /// Invalid password supplied for NEP-2 decryption
    #[error("Invalid password")]
    InvalidPassword,

    /// Generic catch-all error case
    #[error("{message}")]
    Other {
        /// Description of the error
        message: String,
    },

    /// Buffer overflow or underflow
    #[error(
        "Buffer overflow: attempted to read {requested} bytes, but only {available} available"
    )]
    BufferOverflow {
        /// Amount of space requested
        requested: usize,
        /// Amount of space available
        available: usize,
    },

    /// Unexpected end of stream
    #[error("Unexpected end of stream")]
    EndOfStream,

    /// Configuration error
    #[error("Configuration error: {message}")]
    Configuration {
        /// Error message describing the configuration issue
        message: String,
    },

    /// Network-related error
    #[error("Network error: {message}")]
    Network {
        /// Error message describing the network issue
        message: String,
    },

    /// Timeout error
    #[error("Operation timed out after {duration_ms}ms")]
    Timeout {
        /// Duration in milliseconds before timeout
        duration_ms: u64,
    },

    /// Resource not found
    #[error("Resource not found: {resource}")]
    NotFound {
        /// Name of the resource that was not found
        resource: String,
    },

    /// Resource already exists
    #[error("Resource already exists: {resource}")]
    AlreadyExists {
        /// Name of the resource that already exists
        resource: String,
    },

    /// Validation failed
    #[error("Validation failed: {reason}")]
    ValidationFailed {
        /// Reason why validation failed
        reason: String,
    },

    /// Type conversion failed
    #[error("Type conversion failed: cannot convert {from} to {to}")]
    TypeConversion {
        /// Source type name
        from: String,
        /// Target type name
        to: String,
    },

    /// Native contract execution error
    #[error("Native contract error: {message}")]
    NativeContractError {
        /// Detailed description of the native contract failure
        message: String,
    },

    /// Runtime error
    #[error("Runtime error: {message}")]
    RuntimeError {
        /// Description of the runtime failure
        message: String,
    },

    /// Validation error
    #[error("Validation error: {message}")]
    Validation {
        /// Validation error message
        message: String,
    },
}

impl CoreError {
    /// Create a new invalid format error
    pub fn invalid_format<S: Into<String>>(message: S) -> Self {
        Self::InvalidFormat {
            message: message.into(),
        }
    }

    /// Create a new invalid data error
    pub fn invalid_data<S: Into<String>>(message: S) -> Self {
        Self::InvalidData {
            message: message.into(),
        }
    }

    /// Create a new I/O error
    pub fn io<S: Into<String>>(message: S) -> Self {
        Self::Io {
            message: message.into(),
        }
    }

    /// Create a new serialization error
    pub fn serialization<S: Into<String>>(message: S) -> Self {
        Self::Serialization {
            message: message.into(),
        }
    }

    /// Create a new deserialization error
    pub fn deserialization<S: Into<String>>(message: S) -> Self {
        Self::Deserialization {
            message: message.into(),
        }
    }

    /// Create a new invalid operation error
    pub fn invalid_operation<S: Into<String>>(message: S) -> Self {
        Self::InvalidOperation {
            message: message.into(),
        }
    }

    /// Create a new invalid argument error (maps to invalid data internally)
    pub fn invalid_argument<S: Into<String>>(message: S) -> Self {
        Self::InvalidData {
            message: message.into(),
        }
    }

    /// Create a new runtime error
    pub fn runtime_error<S: Into<String>>(message: S) -> Self {
        Self::RuntimeError {
            message: message.into(),
        }
    }

    /// Create a new system error
    pub fn system<S: Into<String>>(message: S) -> Self {
        Self::System {
            message: message.into(),
        }
    }

    /// Create a new insufficient gas error
    pub fn insufficient_gas(required: u64, available: u64) -> Self {
        Self::InsufficientGas {
            required,
            available,
        }
    }

    /// Create a new cryptographic error
    pub fn cryptographic<S: Into<String>>(message: S) -> Self {
        Self::Cryptographic {
            message: message.into(),
        }
    }

    /// Create a new buffer overflow error
    pub fn buffer_overflow(requested: usize, available: usize) -> Self {
        Self::BufferOverflow {
            requested,
            available,
        }
    }

    /// Create a new configuration error
    pub fn configuration<S: Into<String>>(message: S) -> Self {
        Self::Configuration {
            message: message.into(),
        }
    }

    /// Create a new network error
    pub fn network<S: Into<String>>(message: S) -> Self {
        Self::Network {
            message: message.into(),
        }
    }

    /// Create a new timeout error
    pub fn timeout(duration_ms: u64) -> Self {
        Self::Timeout { duration_ms }
    }

    /// Create a new not found error
    pub fn not_found<S: Into<String>>(resource: S) -> Self {
        Self::NotFound {
            resource: resource.into(),
        }
    }

    /// Create a new already exists error
    pub fn already_exists<S: Into<String>>(resource: S) -> Self {
        Self::AlreadyExists {
            resource: resource.into(),
        }
    }

    /// Create a new validation failed error
    pub fn native_contract<S: Into<String>>(message: S) -> Self {
        Self::NativeContractError {
            message: message.into(),
        }
    }

    pub fn validation_failed<S: Into<String>>(reason: S) -> Self {
        Self::ValidationFailed {
            reason: reason.into(),
        }
    }

    /// Create a new type conversion error
    pub fn type_conversion<S: Into<String>>(from: S, to: S) -> Self {
        Self::TypeConversion {
            from: from.into(),
            to: to.into(),
        }
    }

    /// Create a new validation error
    pub fn validation<S: Into<String>>(message: S) -> Self {
        Self::Validation {
            message: message.into(),
        }
    }

    /// Check if this error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            CoreError::Io { .. }
                | CoreError::Network { .. }
                | CoreError::Timeout { .. }
                | CoreError::System { .. }
        )
    }

    /// Check if this error is a user error (vs system error)
    pub fn is_user_error(&self) -> bool {
        matches!(
            self,
            CoreError::InvalidFormat { .. }
                | CoreError::InvalidData { .. }
                | CoreError::InvalidOperation { .. }
                | CoreError::ValidationFailed { .. }
                | CoreError::TypeConversion { .. }
                | CoreError::InsufficientGas { .. }
                | CoreError::Base58Decode { .. }
                | CoreError::InvalidWif
                | CoreError::InvalidPrivateKey
                | CoreError::InvalidNep2Key
                | CoreError::InvalidPassword
        )
    }

    /// Check if this error is a system error
    pub fn is_system_error(&self) -> bool {
        !self.is_user_error()
    }

    /// Get error category for logging/metrics
    pub fn category(&self) -> &'static str {
        match self {
            CoreError::InvalidFormat { .. } | CoreError::InvalidData { .. } => "validation",
            CoreError::Io { .. } | CoreError::Network { .. } => "io",
            CoreError::Serialization { .. } | CoreError::Deserialization { .. } => "serialization",
            CoreError::InvalidOperation { .. } => "operation",
            CoreError::System { .. } => "system",
            CoreError::InsufficientGas { .. } => "resource",
            CoreError::Cryptographic { .. } => "cryptography",
            CoreError::Base58Decode { .. } => "serialization",
            CoreError::InvalidWif
            | CoreError::InvalidPrivateKey
            | CoreError::InvalidNep2Key
            | CoreError::InvalidPassword => "validation",
            CoreError::Scrypt { .. } | CoreError::Aes { .. } => "cryptography",
            CoreError::BufferOverflow { .. } | CoreError::EndOfStream => "buffer",
            CoreError::Configuration { .. } => "configuration",
            CoreError::Timeout { .. } => "timeout",
            CoreError::NotFound { .. } | CoreError::AlreadyExists { .. } => "resource",
            CoreError::ValidationFailed { .. } => "validation",
            CoreError::TypeConversion { .. } => "conversion",
            CoreError::NativeContractError { .. } => "native_contract",
            CoreError::RuntimeError { .. } => "runtime",
            CoreError::Validation { .. } => "validation",
            CoreError::Other { .. } => "unknown",
        }
    }
}

/// Result type for core operations
pub type CoreResult<T> = std::result::Result<T, CoreError>;

/// Generic result alias mirroring `std::result::Result` but defaulting to `CoreError`.
pub type Result<T, E = CoreError> = std::result::Result<T, E>;

// Standard library error conversions using macro to reduce boilerplate
crate::impl_error_from! {
    CoreError,
    std::io::Error => io,
    std::fmt::Error => serialization,
    crate::neo_io::IoError => serialization,
}

impl From<PrimitiveError> for CoreError {
    fn from(error: PrimitiveError) -> Self {
        match error {
            PrimitiveError::InvalidFormat { message } => CoreError::invalid_format(message),
            PrimitiveError::InvalidData { message } => CoreError::invalid_data(message),
            PrimitiveError::BufferOverflow {
                requested,
                available,
            } => CoreError::BufferOverflow {
                requested,
                available,
            },
            PrimitiveError::TypeConversion { from, to } => CoreError::type_conversion(from, to),
        }
    }
}

// Type conversion errors (require custom handling)
impl From<std::num::ParseIntError> for CoreError {
    fn from(_error: std::num::ParseIntError) -> Self {
        CoreError::type_conversion("string", "integer")
    }
}

impl From<std::num::ParseFloatError> for CoreError {
    fn from(_error: std::num::ParseFloatError) -> Self {
        CoreError::type_conversion("string", "float")
    }
}

// UTF-8 errors (require custom message)
impl From<std::string::FromUtf8Error> for CoreError {
    fn from(_error: std::string::FromUtf8Error) -> Self {
        CoreError::invalid_data("invalid UTF-8 sequence")
    }
}

impl From<std::str::Utf8Error> for CoreError {
    fn from(_error: std::str::Utf8Error) -> Self {
        CoreError::invalid_data("invalid UTF-8 sequence")
    }
}

// Removed neo-cryptography dependency - using external crypto crates directly

/// Unified error type that can hold any error from the Neo stack.
///
/// This provides a common error type for cross-crate error handling.
/// For crate-internal errors, use the specific error type (e.g., `CoreError`).
#[derive(Debug)]
pub enum UnifiedError {
    /// Core crate errors
    Core(CoreError),
    /// Standard I/O errors
    Io(std::io::Error),
    /// Generic string errors
    Message(String),
}

impl std::fmt::Display for UnifiedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Core(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Message(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for UnifiedError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Core(e) => Some(e),
            Self::Io(e) => Some(e),
            Self::Message(_) => None,
        }
    }
}

impl From<CoreError> for UnifiedError {
    fn from(e: CoreError) -> Self {
        Self::Core(e)
    }
}

impl From<std::io::Error> for UnifiedError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<String> for UnifiedError {
    fn from(e: String) -> Self {
        Self::Message(e)
    }
}

impl From<&str> for UnifiedError {
    fn from(e: &str) -> Self {
        Self::Message(e.to_string())
    }
}

/// Result type using UnifiedError
pub type UnifiedResult<T> = std::result::Result<T, UnifiedError>;

#[cfg(test)]
#[allow(dead_code)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let error = CoreError::invalid_format("test message");
        assert!(matches!(error, CoreError::InvalidFormat { .. }));
        assert_eq!(error.to_string(), "Invalid format: test message");
    }

    #[test]
    fn test_error_categories() {
        assert_eq!(CoreError::invalid_format("test").category(), "validation");
        assert_eq!(CoreError::io("test").category(), "io");
        assert_eq!(CoreError::cryptographic("test").category(), "cryptography");
    }

    #[test]
    fn test_retryable_errors() {
        assert!(CoreError::network("test").is_retryable());
        assert!(CoreError::timeout(1000).is_retryable());
        assert!(!CoreError::invalid_format("test").is_retryable());
    }

    #[test]
    fn test_user_vs_system_errors() {
        assert!(CoreError::invalid_data("test").is_user_error());
        assert!(!CoreError::invalid_data("test").is_system_error());

        assert!(CoreError::network("test").is_system_error());
        assert!(!CoreError::network("test").is_user_error());
    }

    #[test]
    fn test_insufficient_gas_error() {
        let error = CoreError::insufficient_gas(1000, 500);
        assert_eq!(
            error.to_string(),
            "Insufficient gas: required 1000, available 500"
        );
    }

    #[test]
    fn test_buffer_overflow_error() {
        let error = CoreError::buffer_overflow(100, 50);
        assert_eq!(
            error.to_string(),
            "Buffer overflow: attempted to read 100 bytes, but only 50 available"
        );
    }

    #[test]
    fn test_from_std_errors() {
        let io_error = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let core_error = CoreError::from(io_error);
        assert!(matches!(core_error, CoreError::Io { .. }));

        let parse_error = "abc".parse::<i32>().unwrap_err();
        let core_error = CoreError::from(parse_error);
        assert!(matches!(core_error, CoreError::TypeConversion { .. }));
    }
}
