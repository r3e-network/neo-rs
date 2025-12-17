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
//! use neo_hsm::{HsmConfig, HsmRuntime, HsmDeviceType};
//!
//! // Create HSM runtime with Ledger
//! let config = HsmConfig {
//!     device_type: HsmDeviceType::Ledger,
//!     slot: 0,
//!     ..Default::default()
//! };
//!
//! let runtime = HsmRuntime::new(config).await?;
//! runtime.unlock("1234").await?;
//!
//! // Sign data
//! let signature = runtime.sign("m/44'/888'/0'/0/0", &data).await?;
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
