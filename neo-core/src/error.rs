//! Error types for the Neo Core crate.
//!
//! Shim module (Task 230, D2 phase 1): the authoritative `CoreError`,
//! `CoreResult`, `Result`, and `ToNativeError` definitions live in
//! `neo-smart-contract` and are re-exported here so that all existing
//! `crate::error::*`, `neo_core::error::*`, `neo_core::CoreError`,
//! `neo_core::CoreResult`, and `neo_core::Result` paths keep compiling while
//! unifying `TypeId` to the authoritative type. The previous local duplicate
//! (including the lossy `From<neo_smart_contract::error::CoreError>` bridge
//! that collapsed every variant into `InvalidOperation`) was removed; the
//! conversion is now the std identity `From<T> for T`.
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
//! use neo_core::error::CoreError;
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

pub use neo_smart_contract::error::{CoreError, CoreResult, Result, ToNativeError};
