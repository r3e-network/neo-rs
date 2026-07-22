//! # neo-payloads::validation
//!
//! Validation routines and typed verdicts for protocol data.
//!
//! ## Boundary
//!
//! This module belongs to `neo-payloads`. This protocol crate owns payload
//! records and validation helpers and must not perform IO, storage commits, or
//! service orchestration.
//!
//! ## Contents
//!
//! - `script_validation`: script validation helpers.
//! - `validation`: Validation routines and typed verdicts for protocol data.

/// Strict VM script validation helpers re-exported from `neo-vm`.
pub mod script_validation;
mod validation;

pub use validation::{MAX_TIMESTAMP_DRIFT_MS, MIN_TIMESTAMP_MS};
