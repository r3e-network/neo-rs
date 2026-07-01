//! # neo-primitives::tests
//!
//! Test module grouping Module-local tests and regression coverage. coverage
//! for neo-primitives.
//!
//! ## Boundary
//!
//! This is test/benchmark-only code for neo-primitives; it may assemble
//! fixtures but must not introduce production behavior.
//!
//! ## Contents
//!
//! - `uint160_tests`: UInt160 regression coverage.
//! - `uint256_tests`: UInt256 regression coverage.

/// Unit tests for `UInt160` type.
#[path = "numeric/uint160_tests.rs"]
pub mod uint160_tests;
/// Unit tests for `UInt256` type.
#[path = "numeric/uint256_tests.rs"]
pub mod uint256_tests;
