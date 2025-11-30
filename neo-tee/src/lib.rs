//! NEO TEE (Trusted Execution Environment) Support
//!
//! This crate provides TEE/SGX support for the Neo N3 blockchain, enabling:
//! - Protected wallet storage with sealed keys
//! - Fair transaction ordering to prevent MEV (Miner Extractable Value)
//! - Remote attestation for verifiable TEE execution
//!
//! # Features
//!
//! - `simulation` (default): Run in simulation mode without real SGX hardware
//! - `sgx-hw`: Enable real Intel SGX hardware support
//! - `attestation`: Enable remote attestation (requires `sgx-hw`)
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
pub mod enclave;
pub mod error;
pub mod mempool;
pub mod wallet;

pub use attestation::{AttestationReport, AttestationService};
pub use enclave::{EnclaveConfig, TeeEnclave};
pub use error::{TeeError, TeeResult};
pub use mempool::{FairOrderingPolicy, TeeMempool};
pub use wallet::{SealedKey, TeeWallet, TeeWalletProvider};
