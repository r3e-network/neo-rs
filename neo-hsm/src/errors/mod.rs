//! # neo-hsm::errors
//!
//! Typed errors and result aliases for this crate boundary.
//!
//! ## Boundary
//!
//! This module belongs to `neo-hsm`. This adapter crate owns signing-provider
//! integration and must not own consensus, ledger persistence, or node
//! orchestration.
//!
//! ## Contents
//!
//! - `error`: Typed error definitions and conversions.

pub mod error;

pub use error::{HsmError, HsmResult};
