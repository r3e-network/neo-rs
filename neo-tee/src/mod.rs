//! NEO TEE (Trusted Execution Environment) Support
//!
//! This crate provides TEE/SGX support for the Neo N3 blockchain, enabling:
//! - Protected wallet storage with sealed keys
//! - Fair transaction ordering to prevent MEV (Miner Extractable Value)
//! - Remote attestation for verifiable TEE execution
//!
//! # SGX hardware support status (R16)
//!
//! The `tee-sgx` feature currently provides **quote evidence verification**
//! (DCAP-style checks of MRENCLAVE/MRSIGNER/attributes) and **experimental**
//! host-side sealing. The sealing key and the monotonic counter are managed
//! by the host process; private-key operations execute in the host process.
//! This is NOT hardware-enforced execution and NOT hardware-backed
//! anti-rollback — deployments must not represent it as such.
//!
//! # Features
//!
//! `integration` is enabled by default; every SGX-related feature is off, so a
//! default build fails closed: [`TeeEnclave::initialize`] refuses to derive a
//! sealing key.
//!
//! - `integration` (default): the node runtime assembly (`runtime`) and the
//!   wallet adapter (`wallet_adapter`), plus the integration-only crate edges
//!   (`neo-wallets`, `neo-protocol`, `neo-smart-contract`) they need. Without
//!   it the crate is core TEE only: sealing, attestation and fair ordering.
//! - `tee-sgx`: Enable real Intel SGX hardware support
//! - `attestation`: Enable remote attestation (requires `tee-sgx`)
//! - `tee-sgx-sim`: Compile the software (simulated) sealing key into a build
//!   without `tee-sgx`. Test-only: enable it from `[dev-dependencies]`, never
//!   from `[dependencies]`, because any host process can recompute the key.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      Untrusted Host                         │
//! │  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐  │
//! │  │   neo-cli    │  │   neo-node   │  │   RPC Server     │  │
//! │  └──────┬───────┘  └──────┬───────┘  └────────┬─────────┘  │
//! │         │                 │                   │            │
//! │         └─────────────────┼───────────────────┘            │
//! │                           │                                │
//! │  ┌────────────────────────┴────────────────────────────┐   │
//! │  │                    TEE Bridge                        │   │
//! │  └────────────────────────┬────────────────────────────┘   │
//! ├───────────────────────────┼────────────────────────────────┤
//! │                           │      SGX Enclave               │
//! │  ┌────────────────────────┴────────────────────────────┐   │
//! │  │                  Enclave Runtime                     │   │
//! │  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  │   │
//! │  │  │   Wallet    │  │   Mempool   │  │ Attestation │  │   │
//! │  │  │  (Sealed)   │  │   (Fair)    │  │   Service   │  │   │
//! │  │  └─────────────┘  └─────────────┘  └─────────────┘  │   │
//! │  └─────────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────┘
//! ```

pub mod attestation;
pub mod config;
pub mod enclave;
pub mod error;
pub(crate) mod fs_acl;
pub mod mempool;
// Phase 3 S6c: node tee_integration runtime + tee_wallet adapter sank here,
// gated behind the `integration` feature (default on since Phase 9, F14) along
// with the wallet/protocol/contract edges those modules alone need.
#[cfg(feature = "integration")]
pub mod runtime;
#[cfg(feature = "tee-sgx")]
pub(crate) mod sgx;
pub mod wallet;
#[cfg(feature = "integration")]
pub mod wallet_adapter;

pub use attestation::{AttestationReport, AttestationService};
pub use enclave::{EnclaveConfig, TeeEnclave};
pub use error::{EnclaveInitError, TeeError, TeeResult};
pub use mempool::{FairOrderingPolicy, TeeMempool};
pub use wallet::{SealedKey, TeeWallet, TeeWalletProvider};
