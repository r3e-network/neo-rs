#![deny(unsafe_code)]
#![warn(missing_docs)]

//! # neo-hsm
//!
//! Hardware Security Module (HSM) support for Neo N3 blockchain.
//!
//! This crate provides a unified interface for hardware-based key management
//! and signing operations, supporting:
//!
//! - **Ledger** hardware wallets (Nano S/X/S Plus)
//! - **PKCS#11** generic HSM interface (YubiHSM, SoftHSM, etc.)
//! - **Simulation** mode for testing without hardware
//!
//! ## Features
//!
//! - `simulation` (default): Software simulation for testing
//! - `ledger`: Ledger hardware wallet support via USB HID
//! - `pkcs11`: Generic HSM support via PKCS#11 interface
//!
//! ## Usage
//!
//! ```ignore
//! use neo_hsm::{HsmConfig, HsmDeviceType, HsmSigner, LedgerSigner};
//!
//! // Build a backend signer (LedgerSigner / Pkcs11Signer / SimulationSigner)
//! // from an HsmConfig, then sign through the HsmSigner trait.
//! let config = HsmConfig {
//!     device_type: HsmDeviceType::Ledger,
//!     ..Default::default()
//! };
//! let signer = LedgerSigner::new(config)?;
//! let signature = signer.sign_hash(&key_info, &hash)?;
//! ```

pub mod config;
pub mod device;
pub mod error;
pub mod pin;
pub mod signer;

#[cfg(feature = "ledger")]
pub mod ledger;

#[cfg(feature = "pkcs11")]
pub mod pkcs11;

#[cfg(feature = "simulation")]
pub mod simulation;

// Re-exports
pub use config::HsmConfig;
pub use device::{HsmDeviceInfo, HsmDeviceType};
pub use error::{HsmError, HsmResult};
pub use pin::prompt_pin;
pub use signer::{HsmKeyInfo, HsmSigner};

#[cfg(feature = "ledger")]
pub use ledger::LedgerSigner;

#[cfg(feature = "pkcs11")]
pub use pkcs11::Pkcs11Signer;

#[cfg(feature = "simulation")]
pub use simulation::SimulationSigner;
