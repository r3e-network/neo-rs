# Error Handling Standardization Guide

This guide documents the standardized error handling patterns implemented across all Neo-RS crates in version 0.3.0.

## Overview

Neo-RS now uses a consistent, professional error handling pattern across all crates that provides:

- **Structured Error Types**: Rich error information with context
- **Error Classification**: Retryable, user vs system errors, severity levels
- **Backward Compatibility**: Deprecated legacy error types for smooth migration
- **Comprehensive Conversions**: Standard library and cross-crate error conversions
- **Observability**: Error categories and severity for logging/metrics

## Standardized Pattern

### 1. Error Module Structure

Each crate now has a dedicated `error.rs` module with:

```rust
// crates/{crate}/src/error.rs
use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum {Crate}Error {
    // Structured error variants with rich context
    #[error("Operation failed: {operation}, reason: {reason}")]
    OperationFailed { operation: String, reason: String },
    
    // ... more variants
}

impl {Crate}Error {
    // Helper constructors
    pub fn operation_failed<S: Into<String>>(operation: S, reason: S) -> Self {
        Self::OperationFailed {
            operation: operation.into(),
            reason: reason.into(),
        }
    }
    
    // Classification methods
    pub fn is_retryable(&self) -> bool { /* ... */ }
    pub fn is_user_error(&self) -> bool { /* ... */ }
    pub fn severity(&self) -> ErrorSeverity { /* ... */ }
    pub fn category(&self) -> &'static str { /* ... */ }
}

pub type {Crate}Result<T> = Result<T, {Crate}Error>;
pub type Result<T> = {Crate}Result<T>;
```

### 2. Crate Integration

Each crate's `lib.rs` includes:

```rust
// Module declaration
pub mod error;

// Re-exports
pub use error::{CrateError, CrateResult, Result};

// Legacy compatibility
#[deprecated(since = "0.3.0", note = "Use CrateError instead")]
pub use LegacyError as Error;

#[derive(Error, Debug)]
pub enum LegacyError {
    // Old error variants for backward compatibility
}
```

## Error Types by Crate

### Core Crate (`neo-core`)

**New**: `CoreError` with variants:
- `InvalidFormat { message }` - Invalid data format
- `InvalidData { message }` - Invalid data content  
- `Io { message }` - I/O operation failed
- `InsufficientGas { required, available }` - Gas limit exceeded
- `Cryptographic { message }` - Cryptographic operation failed
- And more...

**Features**:
- Helper constructors: `CoreError::invalid_format("message")`
- Classification: `is_retryable()`, `is_user_error()`, `category()`
- Conversions from `std::io::Error`, `neo_io::Error`, etc.

### VM Crate (`neo-vm`)

**New**: `VmError` with variants:
- `Parse { message }` - Script/instruction parsing failed
- `InvalidInstruction { opcode, reason }` - Invalid instruction
- `StackUnderflow { requested, available }` - Stack underflow
- `GasExhausted { used, limit }` - Gas limit exceeded
- And more...

**Features**:
- VM-specific classification: `should_fault()`, `is_resource_limit()`
- Rich context for debugging VM execution issues
- Backward compatibility with existing `Error` enum

### Network Crate (`neo-network`)

**New**: `NetworkError` with variants:
- `ConnectionFailed { address, reason }` - Connection establishment failed
- `ProtocolViolation { peer, violation }` - Protocol compliance violation
- `RateLimitExceeded { peer, current_rate, limit }` - Rate limiting
- `HandshakeFailed { peer, reason }` - P2P handshake failed
- And more...

**Features**:
- Network-specific classification: `is_connection_error()`, `should_ban_peer()`
- Rich peer context for network debugging
- Error severity levels for operational monitoring

### I/O Crate (`neo-io`)

**New**: `IoError` with variants:
- `Serialization { type_name, reason }` - Serialization failed
- `BufferOverflow { operation, attempted, capacity }` - Buffer limits exceeded
- `EndOfStream { expected, context }` - Unexpected end of data
- `ChecksumMismatch { expected, actual }` - Data integrity check failed
- And more...

**Features**:
- I/O-specific classification: `is_recoverable()`
- Detailed context for serialization debugging
- Stream operation error handling

## Error Classification

### Error Categories

All errors provide a `category()` method for logging/metrics:

```rust
match error.category() {
    "validation" => // User input/data validation errors
    "io" => // I/O operation errors  
    "network" => // Network communication errors
    "resource" => // Resource limit/availability errors
    "cryptography" => // Cryptographic operation errors
    // ... more categories
}
```

### Severity Levels

```rust
pub enum ErrorSeverity {
    Low,      // Minor issues, don't affect functionality
    Medium,   // May affect performance or specific features  
    High,     // Serious issues, significantly impact functionality
    Critical, // Prevent normal operation
}
```

### Classification Methods

```rust
// Retryability - can the operation be retried?
if error.is_retryable() {
    // Implement retry logic
}

// User vs System - is this a user error or system failure?
if error.is_user_error() {
    // Log as user error, return to client
} else {
    // Log as system error, alert operations
}

// Network-specific classifications
if network_error.should_ban_peer() {
    // Disconnect and ban the peer
}

// VM-specific classifications  
if vm_error.should_fault() {
    // Put VM in fault state
}
```

## Migration Guide

### For Application Code

**Old Pattern**:
```rust
use neo_core::Error;
let result: Result<(), Error> = operation();
```

**New Pattern**:
```rust
use neo_core::{CoreError, CoreResult};
let result: CoreResult<()> = operation();
// or use the alias:
let result: neo_core::Result<()> = operation();
```

### Error Handling

**Old Pattern**:
```rust
match error {
    Error::InvalidFormat(msg) => println!("Format error: {}", msg),
    Error::IoError(err) => println!("I/O error: {}", err),
}
```

**New Pattern**:
```rust
match error {
    CoreError::InvalidFormat { message } => println!("Format error: {}", message),
    CoreError::Io { message } => println!("I/O error: {}", message),
}

// Or use classification:
if error.is_retryable() {
    // Retry logic
}
println!("Error category: {}", error.category());
```

### Creating Errors

**Old Pattern**:
```rust
return Err(Error::InvalidFormat("Bad data".to_string()));
```

**New Pattern**:
```rust
return Err(CoreError::invalid_format("Bad data"));
// Or directly:
return Err(CoreError::InvalidFormat { message: "Bad data".to_string() });
```

## Best Practices

### 1. Use Helper Constructors

```rust
// Preferred
CoreError::invalid_format("message")

// Instead of
CoreError::InvalidFormat { message: "message".to_string() }
```

### 2. Provide Rich Context

```rust
// Good - includes context
VmError::stack_underflow(required: 5, available: 2)

// Bad - lacks context  
VmError::StackUnderflow
```

### 3. Use Error Classification

```rust
// Log with appropriate level based on severity
match error.severity() {
    ErrorSeverity::Critical => log::error!("Critical error: {}", error),
    ErrorSeverity::High => log::warn!("High severity error: {}", error),
    ErrorSeverity::Medium => log::info!("Error: {}", error),
    ErrorSeverity::Low => log::debug!("Minor error: {}", error),
}

// Implement retry logic
if error.is_retryable() {
    tokio::time::sleep(Duration::from_secs(1)).await;
    return retry_operation();
}
```

### 4. Error Conversions

```rust
// Standard library errors are automatically converted
let file = std::fs::File::open("path")?; // io::Error -> CoreError

// Cross-crate conversions are provided
let vm_result: VmResult<()> = core_operation()?; // CoreError -> VmError
```

## Testing

### Error Testing

```rust
#[test]
fn test_error_classification() {
    let error = CoreError::invalid_format("test");
    assert!(error.is_user_error());
    assert!(!error.is_retryable());
    assert_eq!(error.category(), "validation");
    assert_eq!(error.severity(), ErrorSeverity::Low);
}

#[test]
fn test_error_display() {
    let error = VmError::stack_underflow(5, 2);
    assert_eq!(
        error.to_string(),
        "Stack underflow: attempted to access 5 items, but only 2 available"
    );
}
```

### Backward Compatibility Testing

```rust
#[test]
fn test_legacy_compatibility() {
    let new_error = CoreError::invalid_format("test");
    let legacy_error: neo_core::Error = new_error.into();
    assert!(matches!(legacy_error, neo_core::Error::InvalidFormat(_)));
}
```

## Monitoring and Observability

### Metrics Collection

```rust
// Collect error metrics by category and severity
metrics::increment_counter!(
    "neo_errors_total", 
    "crate" => "neo-vm",
    "category" => error.category(),
    "severity" => format!("{:?}", error.severity()).to_lowercase()
);
```

### Structured Logging

```rust
log::error!(
    target: "neo::vm",
    error = %error,
    category = error.category(),
    severity = ?error.severity(),
    retryable = error.is_retryable(),
    "VM execution failed"
);
```

## Implementation Status

### ✅ Completed Crates

- **neo-core**: Full standardization with `CoreError`
- **neo-vm**: Full standardization with `VmError`  
- **neo-network**: Full standardization with `NetworkError`
- **neo-io**: Full standardization with `IoError`

### 🚧 In Progress / Remaining Crates

- **neo-consensus**: Needs error module creation
- **neo-ledger**: Needs error module creation
- **neo-cryptography**: Needs error module standardization
- **neo-persistence**: Needs error module creation
- **neo-rpc-server**: Needs error module creation
- **neo-smart-contract**: Needs error module creation
- **neo-wallets**: Needs error module creation
- **neo-config**: Needs error module creation

### 📦 Crates with Existing Good Patterns

These crates already have dedicated error modules and good patterns:
- **bls12_381**: Has comprehensive `BlsError` with good patterns
- **cli**: Has good error handling with `CliError`
- **extensions**: Has `ExtensionError` with good patterns
- **json**: Has `JsonError` with good patterns
- **mpt_trie**: Has `MptError` with good patterns
- **rpc_client**: Has excellent error handling (best reference)

## Future Enhancements

1. **Error Codes**: Consider adding numeric error codes for API compatibility
2. **Error Aggregation**: Support for collecting multiple related errors
3. **Error Context Chain**: Support for error cause chains
4. **Serialization**: Make errors serializable for RPC/API responses
5. **Internationalization**: Support for localized error messages

## Resources

- [Rust Error Handling Book](https://doc.rust-lang.org/book/ch09-00-error-handling.html)
- [thiserror Documentation](https://docs.rs/thiserror/)
- [Error Handling in Rust](https://blog.burntsushi.net/rust-error-handling/)