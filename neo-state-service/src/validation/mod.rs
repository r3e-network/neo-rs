//! # neo-state-service::validation
//!
//! Validation routines and typed verdicts for protocol data.
//!
//! ## Boundary
//!
//! This module belongs to `neo-state-service`. This service crate owns state-
//! root and MPT service behavior and must not own block download, consensus,
//! RPC transport, or UI composition.
//!
//! ## Contents
//!
//! - `verification`: validation verdicts and verification coverage.

pub mod verification;

pub use verification::{StateRootCalculator, Verifier, VerifyOutcome};
