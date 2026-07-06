//! Block validation providing comprehensive security checks.
//!
//! This module implements hardened block validation to prevent various
//! attack vectors including oversized blocks, timestamp manipulation,
//! and merkle root tampering. It is the **pure** validation layer: it
//! operates on `BlockLike` trait objects and `&Witness` references, so
//! it has no dependency on the stateful blockchain service, consensus,
//! native-contract, or storage layers. Stateful verification is handled by
//! the service pipeline before a block is admitted.

mod error;
mod header;
mod integrity;
mod limits;
mod timestamp;
mod witness;

pub use error::BlockValidationError;
pub use timestamp::{MAX_TIMESTAMP_DRIFT_MS, MIN_TIMESTAMP_MS};

/// Stateless block-validation checks.
///
/// The pure validation layer grouped onto a single zero-sized type: every
/// check is an associated function (none carry state), so callers spell them
/// `BlockValidator::validate_*`.
pub struct BlockValidator;

#[cfg(test)]
#[path = "../tests/pipeline/block_validation.rs"]
mod tests;
