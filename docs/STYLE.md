# Neo-rs Code Style Guide

> **Version**: 1.0.0  
> **Last Updated**: 2026-01-28

This document defines the coding standards and conventions for the neo-rs project.

## Table of Contents

- [Code Organization Standards](#code-organization-standards)
- [Documentation Requirements](#documentation-requirements)
- [Naming Conventions](#naming-conventions)
- [Visibility Guidelines](#visibility-guidelines)
- [Formatting Standards](#formatting-standards)
- [Error Handling Patterns](#error-handling-patterns)
- [Testing Standards](#testing-standards)

---

## Code Organization Standards

### File Structure

Each crate should follow this structure:

```
crate-name/
├── Cargo.toml          # Crate manifest with metadata
├── src/
│   ├── lib.rs          # Crate root with module declarations
│   ├── error.rs        # Error types for this crate
│   ├── prelude.rs      # Common re-exports (optional)
│   ├── module_a.rs     # Module implementation
│   ├── module_b/
│   │   ├── mod.rs      # Module entry point
│   │   ├── submod.rs   # Sub-module
│   │   └── types.rs    # Module-specific types
│   └── ...
├── tests/
│   ├── unit/           # Unit tests
│   └── integration/    # Integration tests
└── benches/            # Benchmarks (if applicable)
```

### Module Declaration Order

```rust
// 1. Crate-level attributes and documentation
#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]

//! # Crate Name
//!
//! One-line description of the crate.
//!
//! ## Overview
//!
//! Detailed description of what this crate provides.

// 2. External crate imports (sorted alphabetically)
extern crate std;

// 3. Internal crate self-reference (if needed)
extern crate self as neo_crate_name;

// 4. Module declarations (grouped by category)
// Core modules
pub mod core_module;
pub mod types;

// Feature-gated modules
#[cfg(feature = "runtime")]
pub mod runtime;

// Private modules
mod internal;

// 5. Re-exports (public API)
pub use types::{TypeA, TypeB};

// 6. Private implementation
#[cfg(test)]
mod tests;
```

### Import Organization

```rust
// 1. Standard library (grouped)
use std::collections::HashMap;
use std::sync::Arc;

// 2. External crates (alphabetically)
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;

// 3. Internal workspace crates (alphabetically)
use neo_crypto::Crypto;
use neo_primitives::{UInt160, UInt256};

// 4. Local crate imports
use crate::error::CrateError;
use crate::types::MyType;
```

---

## Documentation Requirements

### Crate-Level Documentation

Every crate must have comprehensive crate-level documentation:

```rust
//! # Neo Crate Name
//!
//! One-sentence summary of the crate's purpose.
//!
//! ## Features
//!
//! List of key features:
//! - **Feature 1**: Description
//! - **Feature 2**: Description
//!
//! ## Architecture
//!
//! Brief overview of the internal architecture.
//!
//! ## Example
//!
//! ```rust
//! use neo_crate_name::TypeName;
//!
//! let instance = TypeName::new();
//! ```
//!
//! ## Feature Flags
//!
//! - `feature-a`: Enables functionality A
//! - `feature-b`: Enables functionality B (disabled by default)
```

### Module-Level Documentation

```rust
//! # Module Name
//!
//! One-line description of the module.
//!
//! ## Purpose
//!
//! Detailed explanation of what this module does.
//!
//! ## Types
//!
//! - [`TypeA`]: Description of TypeA
//! - [`TypeB`]: Description of TypeB
//!
//! ## Example
//!
//! ```rust
//! use crate::module_name::TypeA;
//!
//! let value = TypeA::new();
//! ```
```

### Item Documentation

All public items must be documented:

```rust
/// Short, one-sentence summary.
///
/// Longer description explaining the purpose and behavior.
/// Multiple paragraphs are allowed.
///
/// # Type Parameters
///
/// * `T`: The type of values stored in the container.
///
/// # Examples
///
/// ```rust
/// let instance = MyStruct::new();
/// assert!(instance.is_empty());
/// ```
///
/// # Errors
///
/// Returns an error if...
///
/// # Panics
///
/// Panics if...
///
/// # Safety
///
/// Unsafe usage requirements...
pub struct MyStruct<T> { ... }

impl MyStruct {
    /// Creates a new empty instance.
    ///
    /// # Examples
    ///
    /// ```rust
    /// let instance = MyStruct::new();
    /// ```
    pub fn new() -> Self { ... }
    
    /// Returns the number of elements.
    ///
    /// # Examples
    ///
    /// ```rust
    /// let instance = MyStruct::new();
    /// assert_eq!(instance.len(), 0);
    /// ```
    pub fn len(&self) -> usize { ... }
}
```

### Documentation Tests

All code examples in documentation must be testable:

```rust
/// Basic example (runs as test):
/// ```rust
/// let x = 5;
/// ```
///
/// Example that should panic:
/// ```rust,should_panic
/// panic!("this test will fail");
/// ```
///
/// Example that doesn't run (for demonstration):
/// ```rust,no_run
/// loop {
///     println!("Hello, world!");
/// }
/// ```
///
/// Ignored example (compile-only):
/// ```rust,ignore
/// this_code_does_not_compile();
/// ```
```

---

## Naming Conventions

### General Rules

| Item | Convention | Example |
|------|------------|---------|
| Crates | `kebab-case` | `neo-core`, `neo-primitives` |
| Modules | `snake_case` | `smart_contract`, `block_header` |
| Types (structs, enums, traits) | `PascalCase` | `Block`, `Transaction`, `IVerifiable` |
| Functions | `snake_case` | `get_block`, `verify_witness` |
| Variables | `snake_case` | `block_hash`, `tx_count` |
| Constants | `SCREAMING_SNAKE_CASE` | `MAX_BLOCK_SIZE`, `NEO_DECIMALS` |
| Static variables | `SCREAMING_SNAKE_CASE` | `GLOBAL_CONTEXT` |
| Type parameters | `PascalCase`, single letter preferred | `T`, `K`, `V`, `Item` |
| Lifetimes | `snake_case`, starts with `'` | `'a`, `'de`, `'static` |
| Features | `kebab-case` or `snake_case` | `runtime`, `client`, `rocksdb-storage` |

### C# Compatibility Exceptions

When porting from C#, preserve original names for public API compatibility:

```rust
// C#: public interface IVerifiable
pub trait IVerifiable { ... }  // Keep I-prefix for interfaces

// C#: public class UInt256
pub struct UInt256 { ... }  // Keep PascalCase

// C#: public enum VMState { HALT, FAULT, ... }
pub enum VMState { HALT, FAULT, ... }  // Keep variant names
```

### Boolean Predicates

Boolean-returning functions should read as assertions:

```rust
// ✅ GOOD
pub fn is_empty(&self) -> bool;
pub fn has_witness(&self) -> bool;
pub fn can_verify(&self) -> bool;
pub fn contains_key(&self, key: &K) -> bool;

// ❌ BAD
pub fn empty(&self) -> bool;  // Ambiguous
pub fn verify_possible(&self) -> bool;  // Awkward
```

### Getter/Setter Names

```rust
impl Block {
    // ✅ GOOD: Simple getters don't use 'get_' prefix
    pub fn hash(&self) -> &UInt256;
    pub fn height(&self) -> u32;
    pub fn timestamp(&self) -> u64;
    
    // ✅ GOOD: Setters use 'set_' prefix
    pub fn set_timestamp(&mut self, timestamp: u64);
    
    // ❌ BAD: Redundant 'get_' prefix
    pub fn get_hash(&self) -> &UInt256;
    pub fn get_height(&self) -> u32;
}
```

### Conversion Methods

```rust
// ✅ GOOD: Standard conversion names
pub fn as_bytes(&self) -> &[u8];      // Cheap reference conversion
pub fn to_bytes(&self) -> Vec<u8>;    // Allocating conversion
pub fn into_bytes(self) -> Vec<u8>;   // Consuming conversion
pub fn from_bytes(bytes: &[u8]) -> Self;  // Constructor
pub fn try_from_bytes(bytes: &[u8]) -> Result<Self, Error>;  // Fallible

// ✅ GOOD: Explicit conversion traits
impl From<BlockHeader> for Block { ... }
impl TryFrom<&str> for UInt256 { ... }
impl AsRef<[u8]> for UInt160 { ... }
```

---

## Visibility Guidelines

### Default to Private

```rust
// Start private, expose only what's needed
mod internal {
    // Private by default
    struct InternalHelper;
    
    // Explicitly pub within the module
    pub struct PublicWithinModule;
}

// Public API
pub struct PublicType {
    // Public fields only when necessary
    pub public_field: u32,
    // Private fields by default
    private_field: String,
}

// Crate-public for internal use
pub(crate) fn internal_helper() { ... }

// Super-module visibility
pub(super) fn parent_visible() { ... }
```

### API Surface Minimization

```rust
// ✅ GOOD: Minimal public API
pub mod ledger {
    // Only expose what users need
    pub use block::{Block, BlockHeader};
    pub use transaction::Transaction;
    
    // Keep implementation details private
    mod block {
        pub struct Block { ... }
        pub struct BlockHeader { ... }
        
        // Internal helpers stay private
        fn validate_header(header: &BlockHeader) -> bool { ... }
    }
    
    mod transaction {
        pub struct Transaction { ... }
        
        // Crate-visible for testing
        pub(crate) fn validate_tx(tx: &Transaction) -> bool { ... }
    }
}
```

### Feature-Gated Visibility

```rust
// In lib.rs

/// Core functionality (always available)
pub mod core;

/// Runtime features (requires 'runtime' feature)
#[cfg(feature = "runtime")]
pub mod runtime;

/// Internal utilities (not public API)
#[cfg(feature = "runtime")]
mod runtime_internal;

// In a module

/// Public constructor
impl Block {
    pub fn new(...) -> Self { ... }
}

/// Runtime-only functionality
#[cfg(feature = "runtime")]
impl Block {
    pub async fn validate_async(&self) -> Result<(), Error> { ... }
}
```

---

## Formatting Standards

### Rustfmt Configuration

Use the project's `rustfmt.toml`:

```toml
# rustfmt.toml
edition = "2024"
max_width = 100
use_small_heuristics = "Default"
reorder_imports = true
reorder_modules = true
```

### Line Length

- Maximum 100 characters per line
- Break long lines at logical points

```rust
// ✅ GOOD: Broken at natural boundaries
let result = some_function(
    first_argument,
    second_argument,
    third_argument,
)?;

// ✅ GOOD: Chain broken logically
let value = iterator
    .filter(|x| x.is_valid())
    .map(|x| x.transform())
    .collect::<Vec<_>>();
```

### Trailing Commas

Always use trailing commas in multi-line structures:

```rust
// ✅ GOOD
struct Point {
    x: f64,
    y: f64,  // Trailing comma
}

// ✅ GOOD
let tuple = (
    first,
    second,
    third,  // Trailing comma
);

// ❌ BAD: Missing trailing comma
struct Point {
    x: f64,
    y: f64
}
```

### Match Formatting

```rust
match value {
    // Simple arm on one line
    Pattern::Simple => result,
    
    // Complex arm with block
    Pattern::Complex => {
        let intermediate = compute();
        intermediate.finalize()
    }
    
    // Guard on same line or next line if long
    Pattern::Guarded if condition => result,
    Pattern::LongGuarded if some_very_long_condition_expression => {
        block_result
    }
    
    // Wildcard last
    _ => default_result,
}
```

### Where Clauses

```rust
// Short: inline
fn short<T: Display>(x: T) -> String { ... }

// Medium: on function line
fn medium<T, U>(x: T, y: U) -> Result<T, Error>
where
    T: Display + Debug,
    U: Clone,
{ ... }

// Long: each bound on its own line
fn long<T, U, V>(x: T, y: U, z: V) -> Result<T, Error>
where
    T: Display
        + Debug
        + Serialize
        + DeserializeOwned,
    U: Clone + Default,
    V: Send + Sync + 'static,
{ ... }
```

---

## Error Handling Patterns

### Error Type Definition

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CrateError {
    #[error("Invalid input: {message}")]
    InvalidInput { message: String },
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Cryptographic error: {0}")]
    Crypto(#[from] neo_crypto::CryptoError),
    
    #[error("Other error: {0}")]
    Other(String),
}

pub type CrateResult<T> = Result<T, CrateError>;
```

### The `?` Operator

```rust
// ✅ GOOD: Propagate errors with context
fn process_block(data: &[u8]) -> CrateResult<Block> {
    let header = parse_header(data)?;
    let transactions = parse_transactions(data)?;
    Ok(Block::new(header, transactions))
}

// ✅ GOOD: Map errors for context
fn verify_hash(hash: &UInt256) -> CrateResult<()> {
    some_operation()
        .map_err(|e| CrateError::InvalidInput {
            message: format!("Hash verification failed: {}", e),
        })
}
```

### Avoid Panics in Library Code

```rust
// ✅ GOOD: Return Result instead of panicking
pub fn divide(a: i64, b: i64) -> Result<i64, MathError> {
    if b == 0 {
        return Err(MathError::DivisionByZero);
    }
    Ok(a / b)
}

// ❌ BAD: Don't panic on invalid input
pub fn divide(a: i64, b: i64) -> i64 {
    assert!(b != 0, "division by zero");  // Avoid in library code
    a / b
}
```

### Option Handling

```rust
// ✅ GOOD: Use combinators
let value = maybe_value
    .map(|v| v * 2)
    .unwrap_or(default);

// ✅ GOOD: Early return with `?`
let value = maybe_value?;

// ✅ GOOD: Provide context
let value = maybe_value
    .ok_or_else(|| CrateError::MissingValue { field: "amount" })?;
```

---

## Testing Standards

### Test Organization

```rust
// In src/module.rs

#[cfg(test)]
mod tests {
    use super::*;
    
    // Unit tests
    mod unit {
        use super::*;
        
        #[test]
        fn test_new_block() {
            let block = Block::new(...);
            assert_eq!(block.height(), 0);
        }
    }
    
    // Property-based tests
    mod property {
        use super::*;
        use proptest::prelude::*;
        
        proptest! {
            #[test]
            fn doesnt_crash(bytes: Vec<u8>) {
                let _ = Block::try_from(bytes);
            }
        }
    }
}

// Integration tests in tests/integration/
```

### Test Naming

```rust
// ✅ GOOD: Descriptive test names
#[test]
fn block_hash_computes_correctly() { ... }

#[test]
fn transaction_with_invalid_signature_fails_verification() { ... }

#[test]
fn mempool_rejects_duplicate_transaction() { ... }

// ❌ BAD: Vague names
#[test]
fn test1() { ... }

#[test]
fn it_works() { ... }
```

### Assertions

```rust
use pretty_assertions::assert_eq;

#[test]
fn block_serialization_roundtrip() {
    let original = create_test_block();
    let serialized = original.to_bytes().unwrap();
    let deserialized = Block::from_bytes(&serialized).unwrap();
    
    assert_eq!(original, deserialized);
    assert_ne!(original.hash(), UInt256::zero());
    assert!(serialized.len() > 0);
}

#[test]
#[should_panic(expected = "invalid block version")]
fn invalid_version_panics() {
    Block::with_version(999);  // Invalid version
}
```

### Async Tests

```rust
#[tokio::test]
async fn async_operation_completes() {
    let result = async_operation().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn concurrent_operations_work() {
    let handles: Vec<_> = (0..10)
        .map(|i| tokio::spawn(async move { process(i).await }))
        .collect();
    
    for handle in handles {
        assert!(handle.await.is_ok());
    }
}
```

---

## Unsafe Code Guidelines

### Minimize Unsafe Usage

```rust
// ✅ GOOD: Prefer safe alternatives
let bytes = vec![0u8; 1024];

// ❌ BAD: Unnecessary unsafe
let bytes = unsafe {
    let ptr = std::alloc::alloc(layout);
    Vec::from_raw_parts(ptr, 0, 1024)
};
```

### Document Safety Requirements

```rust
/// # Safety
///
/// The caller must ensure:
/// 1. `ptr` is non-null and properly aligned
/// 2. `ptr` points to valid memory of at least `len` bytes
/// 3. The memory is valid for the lifetime of the returned slice
pub unsafe fn from_raw_parts<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    std::slice::from_raw_parts(ptr, len)
}
```

### Isolate Unsafe Code

```rust
// Wrap unsafe in safe abstractions
pub struct SafeWrapper {
    inner: UnsafeInner,
}

impl SafeWrapper {
    pub fn new() -> Self {
        Self {
            inner: unsafe { UnsafeInner::init() },
        }
    }
}

impl Drop for SafeWrapper {
    fn drop(&mut self) {
        unsafe { self.inner.cleanup() };
    }
}
```

---

## Commit Message Standards

### Format

```
type(scope): subject

body (optional)

footer (optional)
```

### Types

- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `style`: Code style changes (formatting, semicolons, etc.)
- `refactor`: Code refactoring
- `perf`: Performance improvements
- `test`: Adding or updating tests
- `chore`: Build process or auxiliary tool changes

### Examples

```
feat(consensus): implement commit phase timeout handling

fix(vm): correct stack underflow check in DUP opcode

docs(architecture): add layered architecture diagram

refactor(storage): extract storage traits to neo-storage crate

test(crypto): add property-based tests for hash functions
```

---

## Summary

Following these guidelines ensures:

1. **Consistency**: Code looks and feels the same across the codebase
2. **Maintainability**: Clear structure makes changes easier
3. **Safety**: Patterns that prevent common bugs
4. **Compatibility**: C# parity is maintained where required
5. **Documentation**: Code is self-documenting and well-explained

When in doubt, follow existing patterns in the codebase and ask for clarification.
