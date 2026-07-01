//! # neo-tee::enclave
//!
//! Trusted-enclave boundary types and host-call helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-tee`. This adapter crate owns TEE integration
//! and must not define protocol bytes, consensus rules, or storage semantics.
//!
//! ## Contents
//!
//! - `runtime`: Runtime flags, execution context state, and VM-facing support
//!   types.
//! - `sealing`: enclave sealing helpers.

mod runtime;
mod sealing;

pub use runtime::{EnclaveConfig, EnclaveState, InitResult, TeeEnclave};
pub use sealing::{KeyDerivationParams, SealedData, Sealing, SecureKey};
