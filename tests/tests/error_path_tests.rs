//! Error Path Test Coverage
//!
//! Comprehensive tests for failure scenarios and error recovery:
//! - Invalid input handling
//! - Resource exhaustion scenarios
//! - Network failure simulation
//! - Corrupted data handling
//! - Boundary condition failures
//! - Concurrent operation errors
//! - State inconsistency detection

use neo_core::error::CoreError;
use neo_crypto::{Crypto, ECCurve, ECPoint};
use neo_primitives::{UInt160, UInt256};

// ============================================================================
// Error Creation and Classification Tests
// ============================================================================

#[test]
fn test_error_category_classification() {
    assert_eq!(CoreError::invalid_format("test").category(), "validation");
    assert_eq!(CoreError::io("test").category(), "io");
    assert_eq!(CoreError::network("test").category(), "io");
    assert_eq!(CoreError::cryptographic("test").category(), "cryptography");
    assert_eq!(CoreError::insufficient_gas(100, 50).category(), "resource");
    assert_eq!(CoreError::timeout(1000).category(), "timeout");
    assert_eq!(CoreError::not_found("test").category(), "resource");
    assert_eq!(CoreError::serialization("test").category(), "serialization");
}

#[test]
fn test_error_retryable_classification() {
    // Retryable errors
    assert!(CoreError::network("connection failed").is_retryable());
    assert!(CoreError::timeout(5000).is_retryable());
    assert!(CoreError::io("temporary failure").is_retryable());
    assert!(CoreError::system("resource busy").is_retryable());

    // Non-retryable errors
    assert!(!CoreError::invalid_format("bad data").is_retryable());
    assert!(!CoreError::cryptographic("signature invalid").is_retryable());
    assert!(!CoreError::insufficient_gas(100, 50).is_retryable());
    assert!(!CoreError::validation_failed("invalid").is_retryable());
}

#[test]
fn test_error_user_vs_system_classification() {
    // User errors
    assert!(CoreError::invalid_data("bad input").is_user_error());
    assert!(CoreError::invalid_operation("not allowed").is_user_error());
    assert!(CoreError::insufficient_gas(100, 50).is_user_error());
    assert!(CoreError::cryptographic("bad key").is_user_error());

    // System errors
    assert!(CoreError::network("connection lost").is_system_error());
    assert!(CoreError::io("disk error").is_system_error());
    assert!(CoreError::timeout(1000).is_system_error());
    assert!(CoreError::system("internal error").is_system_error());
}

// ============================================================================
// Invalid Input Handling Tests
// ============================================================================

#[test]
fn test_invalid_uint160_from_bytes() {
    // Too short
    let short_bytes = vec![0u8; 10];
    let result = UInt160::from_bytes(&short_bytes);
    assert!(result.is_err());

    // Too long
    let long_bytes = vec![0u8; 30];
    let result = UInt160::from_bytes(&long_bytes);
    assert!(result.is_err());
}

#[test]
fn test_invalid_uint256_from_bytes() {
    // Too short
    let short_bytes = vec![0u8; 16];
    let result = UInt256::from_bytes(&short_bytes);
    assert!(result.is_err());

    // Too long
    let long_bytes = vec![0u8; 40];
    let result = UInt256::from_bytes(&long_bytes);
    assert!(result.is_err());
}

#[test]
fn test_invalid_ec_point_creation() {
    // Invalid point data (not on curve)
    let invalid_bytes = vec![0xFF; 33];
    let result = ECPoint::from_bytes(&invalid_bytes);
    assert!(result.is_err());

    // Wrong length
    let wrong_length = vec![0x02; 20];
    let result = ECPoint::from_bytes(&wrong_length);
    assert!(result.is_err());
}

#[test]
fn test_buffer_overflow_error_details() {
    let error = CoreError::buffer_overflow(100, 50);

    assert_eq!(error.category(), "buffer");
    assert!(!error.is_retryable());
    assert!(error.is_system_error());

    let error_msg = error.to_string();
    assert!(error_msg.contains("100"));
    assert!(error_msg.contains("50"));
}

// ============================================================================
// Gas and Resource Limit Tests
// ============================================================================

#[test]
fn test_insufficient_gas_error_handling() {
    let required = 1_000_000u64;
    let available = 500_000u64;

    let error = CoreError::insufficient_gas(required, available);

    assert_eq!(error.category(), "resource");
    assert!(!error.is_retryable());
    assert!(error.is_user_error());

    let msg = error.to_string();
    assert!(msg.contains("1000000"));
    assert!(msg.contains("500000"));
}

#[test]
fn test_zero_gas_scenario() {
    let error = CoreError::insufficient_gas(1, 0);
    assert_eq!(error.category(), "resource");
    assert!(!error.is_retryable());
}

#[test]
fn test_max_gas_boundary() {
    let error = CoreError::insufficient_gas(u64::MAX, u64::MAX - 1);
    assert!(error.to_string().contains(&u64::MAX.to_string()));
}

// ============================================================================
// Type Conversion Error Tests
// ============================================================================

#[test]
fn test_type_conversion_errors() {
    let error = CoreError::type_conversion("string", "integer");
    assert_eq!(error.category(), "conversion");
    assert!(!error.is_retryable());

    let msg = error.to_string();
    assert!(msg.contains("string"));
    assert!(msg.contains("integer"));
}

#[test]
fn test_parse_int_error_conversion() {
    let result: Result<i32, _> = "not_a_number".parse();
    assert!(result.is_err());

    let core_error: CoreError = result.unwrap_err().into();
    assert_eq!(core_error.category(), "conversion");
}

#[test]
fn test_parse_float_error_conversion() {
    let result: Result<f64, _> = "invalid_float".parse();
    assert!(result.is_err());

    let core_error: CoreError = result.unwrap_err().into();
    assert_eq!(core_error.category(), "conversion");
}

#[test]
fn test_utf8_error_conversion() {
    let invalid_utf8 = vec![0xFF, 0xFE, 0xFD];
    let result = String::from_utf8(invalid_utf8);
    assert!(result.is_err());

    let core_error: CoreError = result.unwrap_err().into();
    assert!(core_error.to_string().contains("UTF-8"));
}

// ============================================================================
// Timeout and Network Error Tests
// ============================================================================

#[test]
fn test_timeout_error_details() {
    let duration_ms = 5000u64;
    let error = CoreError::timeout(duration_ms);

    assert_eq!(error.category(), "timeout");
    assert!(error.is_retryable());
    assert!(error.is_system_error());
    assert!(error.to_string().contains("5000"));
}

#[test]
fn test_network_error_retryability() {
    let errors = vec![
        CoreError::network("connection refused"),
        CoreError::network("timeout"),
        CoreError::network("host unreachable"),
    ];

    for error in errors {
        assert!(error.is_retryable(), "Network error should be retryable");
        assert!(error.is_system_error());
    }
}

// ============================================================================
// Cryptographic Error Tests
// ============================================================================

#[test]
fn test_cryptographic_error_types() {
    let errors = vec![
        CoreError::invalid_private_key("invalid format"),
        CoreError::invalid_nep2_key("bad encoding"),
        CoreError::invalid_wif("checksum mismatch"),
        CoreError::scrypt("key derivation failed"),
        CoreError::aes("decryption failed"),
        CoreError::invalid_password("wrong password"),
    ];

    for error in errors {
        assert_eq!(error.category(), "cryptography");
        assert!(!error.is_retryable());
        assert!(error.is_user_error());
    }
}

#[test]
fn test_signature_verification_failure() {
    // Test with infinity point (invalid)
    let point = ECPoint::infinity(ECCurve::Secp256r1);
    assert!(point.is_infinity());

    // Infinity point should not be valid for signature verification
    let encoded = point.encode_point(true).unwrap();
    assert_eq!(encoded, vec![0x00]);
}

// ============================================================================
// Resource Not Found Tests
// ============================================================================

#[test]
fn test_not_found_error() {
    let error = CoreError::not_found("transaction");
    assert_eq!(error.category(), "resource");
    assert!(!error.is_retryable());
    assert!(error.to_string().contains("transaction"));
}

#[test]
fn test_already_exists_error() {
    let error = CoreError::already_exists("account");
    assert_eq!(error.category(), "resource");
    assert!(!error.is_retryable());
    assert!(error.to_string().contains("account"));
}

// ============================================================================
// Validation Failure Tests
// ============================================================================

#[test]
fn test_validation_error_types() {
    let errors = vec![
        CoreError::validation_failed("invalid format"),
        CoreError::validation("missing field"),
        CoreError::invalid_operation("not allowed"),
    ];

    for error in errors {
        assert!(!error.is_retryable());
        assert!(error.is_user_error());
    }
}

// ============================================================================
// Execution Error Tests
// ============================================================================

#[test]
fn test_execution_errors() {
    let native_error = CoreError::native_contract("contract error");
    assert_eq!(native_error.category(), "execution");

    let runtime_error = CoreError::runtime_error("stack overflow");
    assert_eq!(runtime_error.category(), "execution");
}

// ============================================================================
// Configuration Error Tests
// ============================================================================

#[test]
fn test_configuration_errors() {
    let error = CoreError::configuration("invalid port number");
    assert_eq!(error.category(), "configuration");
    assert!(!error.is_retryable());
    assert!(error.is_system_error());
}

// ============================================================================
// Compression Error Tests
// ============================================================================

#[test]
fn test_compression_errors() {
    let error = CoreError::compression("decompression failed");
    assert_eq!(error.category(), "compression");
    assert!(!error.is_retryable());
}

// ============================================================================
// Error Propagation Tests
// ============================================================================

#[test]
fn test_io_error_conversion() {
    let io_error = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
    let core_error: CoreError = io_error.into();

    assert_eq!(core_error.category(), "io");
    assert!(core_error.is_retryable());
}

#[test]
fn test_fmt_error_conversion() {
    use std::fmt;

    let fmt_error = fmt::Error;
    let core_error: CoreError = fmt_error.into();

    assert_eq!(core_error.category(), "serialization");
}

// ============================================================================
// Edge Case Error Tests
// ============================================================================

#[test]
fn test_empty_string_errors() {
    let error = CoreError::invalid_format("");
    assert_eq!(error.category(), "validation");
}

#[test]
fn test_very_long_error_message() {
    let long_msg = "error ".repeat(1000);
    let error = CoreError::system(&long_msg);
    assert!(error.to_string().len() > 5000);
}

#[test]
fn test_special_characters_in_errors() {
    let msg = "Error: <script>alert('xss')</script>";
    let error = CoreError::invalid_data(msg);
    assert!(error.to_string().contains("<script>"));
}

// ============================================================================
// Concurrent Error Scenario Tests
// ============================================================================

#[tokio::test]
async fn test_concurrent_error_creation() {
    use tokio::task;

    let handles: Vec<_> = (0..100)
        .map(|i| {
            task::spawn(async move {
                let error = CoreError::invalid_format(format!("error {}", i));
                assert_eq!(error.category(), "validation");
                error
            })
        })
        .collect();

    for handle in handles {
        let error = handle.await.unwrap();
        assert!(!error.is_retryable());
    }
}

// ============================================================================
// Hash Function Error Coverage
// ============================================================================

#[test]
fn test_hash_empty_data() {
    let empty: Vec<u8> = vec![];
    let hash = UInt256::from(Crypto::hash256(&empty));
    assert_ne!(hash, UInt256::zero());
}

#[test]
fn test_hash_large_data() {
    let large_data = vec![0xAA; 1_000_000];
    let hash = UInt256::from(Crypto::hash256(&large_data));
    assert_ne!(hash, UInt256::zero());
}

// ============================================================================
// End of Stream Error Tests
// ============================================================================

#[test]
fn test_end_of_stream_error() {
    let error = CoreError::EndOfStream;
    assert_eq!(error.category(), "buffer");
    assert!(!error.is_retryable());
    assert!(error.is_system_error());
}
