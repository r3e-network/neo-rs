//! Error types for Neo smart-contract and service operations.
//!
//! This module provides comprehensive error handling for core Neo operations,
//! including type conversions, serialization, and system-level errors.
//!
//! ## Error Categories
//!
//! | Category | Description |
//! |----------|-------------|
//! | `validation` | Invalid format, data, or operation |
//! | `io` | I/O and network errors |
//! | `serialization` | Serialization/deserialization errors |
//! | `cryptography` | Cryptographic operation failures |
//! | `resource` | Gas, memory, or buffer limits exceeded |
//!
//! ## Example
//!
//! ```rust
//! use neo_smart_contract::error::CoreError;
//!
//! // Create an error
//! let err = CoreError::invalid_format("invalid address");
//!
//! // Check error category
//! assert_eq!(err.category(), "validation");
//!
//! // Check if retryable
//! assert!(!err.is_retryable());
//! ```

use neo_primitives::PrimitiveError;
use thiserror::Error;

/// Core module errors
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum CoreError {
    /// Invalid format or data error
    #[error("Invalid: {message}")]
    Invalid {
        /// Error message describing the issue
        message: String,
    },

    /// I/O operation failed
    #[error("I/O error: {message}")]
    Io {
        /// Error message describing the I/O issue
        message: String,
    },

    /// Serialization or deserialization failed
    #[error("Codec error: {message}")]
    Codec {
        /// Error message describing the codec issue
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

    /// Buffer overflow or underflow
    #[error("Buffer overflow: attempted to read {requested} bytes, but only {available} available")]
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

    /// Execution error (native contract or runtime)
    #[error("Execution error: {message}")]
    Execution {
        /// Description of the execution failure
        message: String,
    },

    /// Compression/decompression error
    #[error("Compression error: {message}")]
    Compression {
        /// Error message describing the compression issue
        message: String,
    },
}

impl CoreError {
    /// Create a new invalid format error
    pub fn invalid_format<S: Into<String>>(message: S) -> Self {
        Self::Invalid {
            message: message.into(),
        }
    }

    /// Create a new invalid data error
    pub fn invalid_data<S: Into<String>>(message: S) -> Self {
        Self::Invalid {
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
        Self::Codec {
            message: message.into(),
        }
    }

    /// Create a new deserialization error
    pub fn deserialization<S: Into<String>>(message: S) -> Self {
        Self::Codec {
            message: message.into(),
        }
    }

    /// Create a new invalid operation error
    pub fn invalid_operation<S: Into<String>>(message: S) -> Self {
        Self::InvalidOperation {
            message: message.into(),
        }
    }

    /// Create a new invalid argument error (maps to invalid internally)
    pub fn invalid_argument<S: Into<String>>(message: S) -> Self {
        Self::Invalid {
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

    /// Create a new invalid private key error
    pub fn invalid_private_key<S: Into<String>>(msg: S) -> Self {
        Self::Cryptographic {
            message: msg.into(),
        }
    }

    /// Create a new invalid NEP-2 key error
    pub fn invalid_nep2_key<S: Into<String>>(msg: S) -> Self {
        Self::Cryptographic {
            message: msg.into(),
        }
    }

    /// Create a new invalid WIF error
    pub fn invalid_wif<S: Into<String>>(msg: S) -> Self {
        Self::Cryptographic {
            message: msg.into(),
        }
    }

    /// Create a new scrypt error
    pub fn scrypt<S: Into<String>>(msg: S) -> Self {
        Self::Cryptographic {
            message: msg.into(),
        }
    }

    /// Create a new AES encryption error
    pub fn aes<S: Into<String>>(msg: S) -> Self {
        Self::Cryptographic {
            message: msg.into(),
        }
    }

    /// Create a new invalid password error
    pub fn invalid_password<S: Into<String>>(msg: S) -> Self {
        Self::Cryptographic {
            message: msg.into(),
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

    /// Create a new execution error (native contract or runtime)
    pub fn native_contract<S: Into<String>>(message: S) -> Self {
        Self::Execution {
            message: message.into(),
        }
    }

    /// Create a new execution error
    pub fn runtime_error<S: Into<String>>(message: S) -> Self {
        Self::Execution {
            message: message.into(),
        }
    }

    /// Create a new validation failure error
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

    /// Create a new validation error (delegates to `ValidationFailed`)
    pub fn validation<S: Into<String>>(message: S) -> Self {
        Self::ValidationFailed {
            reason: message.into(),
        }
    }

    /// Create a new compression error
    pub fn compression<S: Into<String>>(message: S) -> Self {
        Self::Compression {
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
            CoreError::Invalid { .. }
                | CoreError::InvalidOperation { .. }
                | CoreError::ValidationFailed { .. }
                | CoreError::TypeConversion { .. }
                | CoreError::InsufficientGas { .. }
                | CoreError::Base58Decode { .. }
                | CoreError::Cryptographic { .. }
        )
    }

    /// Check if this error is a system error
    pub fn is_system_error(&self) -> bool {
        !self.is_user_error()
    }

    /// Get error category for logging/metrics
    pub fn category(&self) -> &'static str {
        match self {
            CoreError::Invalid { .. } => "validation",
            CoreError::Io { .. } | CoreError::Network { .. } => "io",
            CoreError::Codec { .. } => "serialization",
            CoreError::InvalidOperation { .. } => "operation",
            CoreError::System { .. } => "system",
            CoreError::InsufficientGas { .. } => "resource",
            CoreError::Cryptographic { .. } => "cryptography",
            CoreError::Base58Decode { .. } => "serialization",
            CoreError::BufferOverflow { .. } | CoreError::EndOfStream => "buffer",
            CoreError::Configuration { .. } => "configuration",
            CoreError::Timeout { .. } => "timeout",
            CoreError::NotFound { .. } | CoreError::AlreadyExists { .. } => "resource",
            CoreError::ValidationFailed { .. } => "validation",
            CoreError::TypeConversion { .. } => "conversion",
            CoreError::Execution { .. } => "execution",
            CoreError::Compression { .. } => "compression",
        }
    }
}

/// Result type for core operations
pub type CoreResult<T> = std::result::Result<T, CoreError>;

/// Generic result alias mirroring `std::result::Result` but defaulting to `CoreError`.
pub type Result<T, E = CoreError> = std::result::Result<T, E>;

/// Extension trait to convert any `Result<T, E: ToString>` into a
/// `CoreResult<T>` via [`CoreError::native_contract`].
///
/// This eliminates the repetitive `.map_err(CoreError::native_contract)?`
/// pattern found throughout native contract implementations.
pub trait ToNativeError<T> {
    /// Converts this result into a `CoreResult`, mapping any error via
    /// [`CoreError::native_contract`].
    fn native_err(self) -> CoreResult<T>;
}

impl<T, E: ToString> ToNativeError<T> for Result<T, E> {
    fn native_err(self) -> CoreResult<T> {
        self.map_err(|e| CoreError::native_contract(e.to_string()))
    }
}

impl From<std::io::Error> for CoreError {
    fn from(error: std::io::Error) -> Self {
        CoreError::io(error.to_string())
    }
}

impl From<std::fmt::Error> for CoreError {
    fn from(error: std::fmt::Error) -> Self {
        CoreError::serialization(error.to_string())
    }
}

impl From<neo_io::IoError> for CoreError {
    fn from(error: neo_io::IoError) -> Self {
        CoreError::serialization(error.to_string())
    }
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

impl From<neo_storage::StorageError> for CoreError {
    fn from(err: neo_storage::StorageError) -> Self {
        CoreError::InvalidOperation {
            message: err.to_string(),
        }
    }
}

impl From<neo_vm::VmError> for CoreError {
    fn from(err: neo_vm::VmError) -> Self {
        CoreError::InvalidOperation {
            message: err.to_string(),
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

#[cfg(test)]
#[allow(dead_code)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let error = CoreError::invalid_format("test message");
        assert!(matches!(error, CoreError::Invalid { .. }));
        assert_eq!(error.to_string(), "Invalid: test message");
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

    #[test]
    fn all_variants_preserve_display_category_and_classification() {
        let cases = [
            (
                CoreError::invalid_format("x"),
                "Invalid: x",
                "validation",
                false,
                true,
            ),
            (CoreError::io("x"), "I/O error: x", "io", true, false),
            (
                CoreError::serialization("x"),
                "Codec error: x",
                "serialization",
                false,
                false,
            ),
            (
                CoreError::invalid_operation("x"),
                "Invalid operation: x",
                "operation",
                false,
                true,
            ),
            (
                CoreError::system("x"),
                "System error: x",
                "system",
                true,
                false,
            ),
            (
                CoreError::insufficient_gas(9, 2),
                "Insufficient gas: required 9, available 2",
                "resource",
                false,
                true,
            ),
            (
                CoreError::cryptographic("x"),
                "Cryptographic error: x",
                "cryptography",
                false,
                true,
            ),
            (
                CoreError::Base58Decode {
                    message: "x".into(),
                },
                "Base58 decoding error: x",
                "serialization",
                false,
                true,
            ),
            (
                CoreError::buffer_overflow(9, 2),
                "Buffer overflow: attempted to read 9 bytes, but only 2 available",
                "buffer",
                false,
                false,
            ),
            (
                CoreError::EndOfStream,
                "Unexpected end of stream",
                "buffer",
                false,
                false,
            ),
            (
                CoreError::configuration("x"),
                "Configuration error: x",
                "configuration",
                false,
                false,
            ),
            (
                CoreError::network("x"),
                "Network error: x",
                "io",
                true,
                false,
            ),
            (
                CoreError::timeout(9),
                "Operation timed out after 9ms",
                "timeout",
                true,
                false,
            ),
            (
                CoreError::not_found("x"),
                "Resource not found: x",
                "resource",
                false,
                false,
            ),
            (
                CoreError::already_exists("x"),
                "Resource already exists: x",
                "resource",
                false,
                false,
            ),
            (
                CoreError::validation_failed("x"),
                "Validation failed: x",
                "validation",
                false,
                true,
            ),
            (
                CoreError::type_conversion("a", "b"),
                "Type conversion failed: cannot convert a to b",
                "conversion",
                false,
                true,
            ),
            (
                CoreError::native_contract("x"),
                "Execution error: x",
                "execution",
                false,
                false,
            ),
            (
                CoreError::compression("x"),
                "Compression error: x",
                "compression",
                false,
                false,
            ),
        ];
        for (error, display, category, retryable, user_error) in cases {
            assert_eq!(error.to_string(), display);
            assert_eq!(error.category(), category);
            assert_eq!(error.is_retryable(), retryable);
            assert_eq!(error.is_user_error(), user_error);
            assert_eq!(error.is_system_error(), !user_error);
            assert_eq!(error.clone(), error);
        }
    }

    #[test]
    fn constructor_aliases_preserve_variants_and_fields() {
        assert_eq!(CoreError::invalid_data("x"), CoreError::invalid_format("x"));
        assert_eq!(
            CoreError::invalid_argument("x"),
            CoreError::invalid_format("x")
        );
        assert_eq!(
            CoreError::deserialization("x"),
            CoreError::serialization("x")
        );
        for error in [
            CoreError::invalid_private_key("x"),
            CoreError::invalid_nep2_key("x"),
            CoreError::invalid_wif("x"),
            CoreError::scrypt("x"),
        ] {
            assert_eq!(error, CoreError::cryptographic("x"));
        }
        assert_eq!(
            CoreError::runtime_error("x"),
            CoreError::native_contract("x")
        );
        assert_eq!(
            CoreError::validation("x"),
            CoreError::validation_failed("x")
        );
    }

    #[test]
    fn primitive_conversions_preserve_structured_fields() {
        let cases = [
            (
                PrimitiveError::invalid_format("format"),
                CoreError::invalid_format("format"),
            ),
            (
                PrimitiveError::invalid_data("data"),
                CoreError::invalid_data("data"),
            ),
            (
                PrimitiveError::buffer_overflow(23, 7),
                CoreError::BufferOverflow {
                    requested: 23,
                    available: 7,
                },
            ),
            (
                PrimitiveError::type_conversion("source", "target"),
                CoreError::TypeConversion {
                    from: "source".into(),
                    to: "target".into(),
                },
            ),
        ];
        for (source, expected) in cases {
            assert_eq!(CoreError::from(source), expected);
        }
    }

    #[test]
    fn io_conversion_preserves_codec_prefix() {
        let sources = [
            neo_io::IoError::Format,
            neo_io::IoError::InvalidUtf8,
            neo_io::IoError::InvalidData {
                context: "script".into(),
                value: "oversized".into(),
            },
            neo_io::IoError::Io(std::io::Error::other("write failed")),
        ];
        for source in sources {
            let message = source.to_string();
            let error = CoreError::from(source);
            assert_eq!(error.to_string(), format!("Codec error: {message}"));
            assert_eq!(error, CoreError::Codec { message });
            assert_eq!(error.category(), "serialization");
            assert!(!error.is_retryable());
        }
    }

    #[test]
    fn vm_and_storage_conversions_preserve_invalid_operation_text() {
        let vm_error = neo_vm::VmError::invalid_operation_msg("vm failure");
        let vm_message = vm_error.to_string();
        assert_eq!(
            CoreError::from(vm_error),
            CoreError::InvalidOperation {
                message: vm_message
            }
        );

        for source in [
            neo_storage::StorageError::ReadOnly,
            neo_storage::StorageError::key_not_found("key"),
            neo_storage::StorageError::serialization("bytes"),
            neo_storage::StorageError::backend("backend"),
            neo_storage::StorageError::invalid_operation("operation"),
            neo_storage::StorageError::commit_failed("commit"),
            neo_storage::StorageError::io("io"),
        ] {
            let message = source.to_string();
            assert_eq!(
                CoreError::from(source),
                CoreError::InvalidOperation { message }
            );
        }
    }

    #[test]
    fn remaining_std_conversions_preserve_messages() {
        assert_eq!(
            CoreError::from(std::fmt::Error),
            CoreError::serialization(std::fmt::Error.to_string())
        );
        assert_eq!(
            CoreError::from("abc".parse::<f64>().unwrap_err()),
            CoreError::type_conversion("string", "float")
        );
        let bytes = vec![0xff];
        assert_eq!(
            CoreError::from(String::from_utf8(bytes.clone()).unwrap_err()),
            CoreError::invalid_data("invalid UTF-8 sequence")
        );
        assert_eq!(
            CoreError::from(std::str::from_utf8(&bytes).unwrap_err()),
            CoreError::invalid_data("invalid UTF-8 sequence")
        );
    }

    #[test]
    fn native_error_adapter_and_result_aliases_preserve_execution_variant() {
        let result: Result<(), &str> = Err("native failure");
        let mapped: CoreResult<()> = result.native_err();
        assert_eq!(mapped, Err(CoreError::native_contract("native failure")));
        let result: Result<u8> = Ok(7);
        assert_eq!(result.native_err(), Ok(7));
    }
}
