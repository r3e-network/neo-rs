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

pub mod enclave;
pub mod wallet;
pub mod mempool;
pub mod attestation;
pub mod error;

pub use enclave::{TeeEnclave, EnclaveConfig};
pub use wallet::{TeeWallet, TeeWalletProvider, SealedKey};
pub use mempool::{TeeMempool, FairOrderingPolicy};
pub use attestation::{AttestationReport, AttestationService};
pub use error::{TeeError, TeeResult};
