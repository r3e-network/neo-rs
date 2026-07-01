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
fn test_from_neo_io_errors_preserves_error_category() {
    let format_error = CoreError::from(neo_io::IoError::Format);
    assert!(matches!(format_error, CoreError::Codec { .. }));
    assert_eq!(format_error.category(), "serialization");

    let io_error = CoreError::from(neo_io::IoError::Io(std::io::Error::new(
        std::io::ErrorKind::BrokenPipe,
        "socket closed",
    )));
    assert!(matches!(io_error, CoreError::Io { .. }));
    assert_eq!(io_error.category(), "io");
}
