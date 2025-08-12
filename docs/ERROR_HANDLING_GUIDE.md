# Error Handling Implementation Guide

## Overview

This guide documents the new error handling patterns implemented to replace unsafe `unwrap()`, `expect()`, and `panic!` patterns throughout the Neo Rust codebase.

## Key Components

### 1. Error Handling Module (`crates/core/src/error_handling.rs`)

Provides comprehensive error types and utilities:

- **`NeoError`**: Main error enum with domain-specific variants
- **`ErrorContext`**: Trait for adding context to errors
- **`SafeUnwrap`**: Safe alternatives to unwrap()
- **`RetryPolicy`**: Retry mechanism for transient failures
- **`CircuitBreaker`**: Prevents cascading failures

### 2. Safe Operations Module (`crates/core/src/safe_operations.rs`)

Provides safe alternatives to panic-prone operations:

- **`SafeIndex`**: Array access without bounds panics
- **`SafeMap`**: HashMap operations with proper error handling
- **`SafeArithmetic`**: Overflow/underflow protection
- **`SafeMutex`/`SafeRwLock`**: Poison recovery for sync primitives
- **`SafeParse`**: String parsing without panics
- **`SafeConvert`**: Type conversions with overflow protection

## Migration Strategy

### Phase 1: Replace Critical unwrap() Calls

```rust
// Before
let value = some_option.unwrap();
let result = some_result.unwrap();

// After
let value = some_option.ok_or(NeoError::NotFound("value".to_string()))?;
let result = some_result.context("Failed to get result")?;
```

### Phase 2: Replace expect() with Context

```rust
// Before
let data = file.read().expect("Failed to read file");

// After
let data = file.read()
    .context("Failed to read file")?;
```

### Phase 3: Replace panic! with Errors

```rust
// Before
if condition {
    panic!("Invalid state");
}

// After
if condition {
    return Err(NeoError::Internal("Invalid state".to_string()));
}
```

### Phase 4: Safe Arithmetic Operations

```rust
// Before
let sum = a + b; // Can overflow

// After
let sum = a.safe_add(b)?;
```

## Usage Examples

### Error Context

```rust
use crate::error_handling::{ErrorContext, Result};

fn process_block(block_id: u32) -> Result<Block> {
    get_block(block_id)
        .context(format!("Failed to process block {}", block_id))?
}
```

### Safe Operations

```rust
use crate::safe_operations::{SafeIndex, SafeArithmetic};

fn calculate_fee(amounts: &[u64]) -> Result<u64> {
    let base = amounts.safe_get(0).copied().unwrap_or(0);
    let extra = amounts.safe_get(1).copied().unwrap_or(0);
    
    base.safe_add(extra)
}
```

### Retry Policy

```rust
use crate::error_handling::RetryPolicy;

async fn connect_to_peer(addr: &str) -> Result<Connection> {
    let policy = RetryPolicy::new(3, 1000);
    
    policy.retry(|| async {
        establish_connection(addr).await
    }).await
}
```

### Circuit Breaker

```rust
use crate::error_handling::CircuitBreaker;

let breaker = CircuitBreaker::new(5, 3, 60000);

async fn call_external_service() -> Result<Response> {
    breaker.call(|| async {
        external_api_call().await
    }).await
}
```

## Best Practices

1. **Always propagate errors** - Use `?` operator instead of unwrap()
2. **Add context** - Use `.context()` to add meaningful error messages
3. **Use domain-specific errors** - Create specific error variants for different failure modes
4. **Log at appropriate levels** - Use tracing for warnings and errors
5. **Fail gracefully** - Provide defaults or recovery mechanisms where possible
6. **Test error paths** - Write tests for error conditions

## Performance Considerations

- Error handling adds minimal overhead in the success path
- Context strings are only allocated on error
- Retry and circuit breaker patterns prevent resource exhaustion
- Safe operations use efficient checked arithmetic

## Integration with Existing Code

The new error handling is designed to be incrementally adopted:

1. Start with critical paths (consensus, networking, storage)
2. Gradually replace unwrap() calls in less critical code
3. Add tests for error conditions
4. Monitor error metrics in production

## Metrics and Monitoring

Track these metrics:

- Error rates by type
- Retry attempts and success rates
- Circuit breaker state changes
- Recovery success rates

## Next Steps

1. Apply error handling patterns to VM execution
2. Implement error recovery in consensus mechanism
3. Add error metrics collection
4. Create error handling lints for CI/CD
5. Train team on new patterns

## References

- [Rust Error Handling](https://doc.rust-lang.org/book/ch09-00-error-handling.html)
- [Error Design Patterns](https://rust-unofficial.github.io/patterns/idioms/errors.html)
- [Circuit Breaker Pattern](https://martinfowler.com/bliki/CircuitBreaker.html)