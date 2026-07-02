//! # neo-tee
//!
//! Trusted-execution helpers for attestation, enclave integration, and secure
//! signing.
//!
//! ## Boundary
//!
//! This adapter crate owns TEE integration and must not define protocol bytes,
//! consensus rules, or storage semantics.
//!
//! ## Contents
//!
//! - `attestation`: TEE attestation evidence and verification helpers.
//! - `enclave`: Trusted-enclave boundary types and host-call helpers.
//! - `error`: Typed error definitions and conversions.
//! - `mempool`: TEE-facing mempool request helpers.
//! - `nitro`: AWS Nitro enclave integration helpers.
//! - `sgx`: Intel SGX integration.
//! - `wallet`: wallet RPC client methods.

pub mod attestation;
pub mod enclave;
#[path = "errors/error.rs"]
pub mod error;
pub mod mempool;
/// AWS Nitro Enclaves backend (EXPERIMENTAL, `nitro` feature, off by default).
#[cfg(feature = "nitro")]
pub mod nitro;
mod ordering_merkle;
#[cfg(feature = "sgx-hw")]
#[path = "hardware/sgx.rs"]
pub(crate) mod sgx;
pub mod wallet;

pub use attestation::{AttestationReport, AttestationService};
pub use enclave::{EnclaveConfig, TeeEnclave};
pub use error::{TeeError, TeeResult};
pub use mempool::{FairOrderingPolicy, TeeMempool};
pub use wallet::{SealedKey, TeeWallet, TeeWalletProvider};
