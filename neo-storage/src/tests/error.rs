use super::*;

#[test]
fn test_key_not_found_error() {
    let err = StorageError::key_not_found("test_key");
    assert!(matches!(err, StorageError::KeyNotFound { .. }));
    assert!(err.to_string().contains("test_key"));
    assert!(err.to_string().contains("Key not found"));
}

#[test]
fn test_serialization_error() {
    let err = StorageError::serialization("invalid format");
    assert!(matches!(err, StorageError::Serialization { .. }));
    assert!(err.to_string().contains("invalid format"));
    assert!(err.to_string().contains("Serialization error"));
}

#[test]
fn test_backend_error() {
    let err = StorageError::backend("connection failed");
    assert!(matches!(err, StorageError::Backend { .. }));
    assert!(err.to_string().contains("connection failed"));
    assert!(err.to_string().contains("Storage backend error"));
}

#[test]
fn test_invalid_operation_error() {
    let err = StorageError::invalid_operation("cannot write to read-only store");
    assert!(matches!(err, StorageError::InvalidOperation { .. }));
    assert!(err.to_string().contains("cannot write to read-only store"));
    assert!(err.to_string().contains("Invalid operation"));
}

#[test]
fn test_read_only_error() {
    let err = StorageError::ReadOnly;
    assert_eq!(err.to_string(), "Storage is read-only");
}

#[test]
fn test_error_equality() {
    let err1 = StorageError::key_not_found("key1");
    let err2 = StorageError::key_not_found("key1");
    let err3 = StorageError::key_not_found("key2");

    assert_eq!(err1, err2);
    assert_ne!(err1, err3);
}

#[test]
fn test_error_clone() {
    let err1 = StorageError::backend("test");
    let err2 = err1.clone();
    assert_eq!(err1, err2);
}

#[test]
fn test_error_debug() {
    let err = StorageError::ReadOnly;
    let debug_str = format!("{:?}", err);
    assert!(debug_str.contains("ReadOnly"));
}

#[test]
fn test_storage_result_ok() {
    let result: StorageResult<i32> = Ok(42);
    assert_eq!(result, Ok(42));
}

#[test]
fn test_storage_result_err() {
    let result: StorageResult<i32> = Err(StorageError::ReadOnly);
    assert_eq!(result, Err(StorageError::ReadOnly));
}
