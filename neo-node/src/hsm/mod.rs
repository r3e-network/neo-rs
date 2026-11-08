//! Hardware Security Module (HSM) support for Neo N3 blockchain.
//!
//! This module provides a unified interface for hardware-based key management
//! and signing operations, supporting:
//!
//! - **Ledger** hardware wallets (Nano S/X/S Plus)
//! - **PKCS#11** generic HSM interface (YubiHSM, SoftHSM, etc.)
//!
//! ## Features
//!
//! - `hsm`: Enable HSM support
//! - `hsm-ledger`: Ledger hardware wallet support via USB HID
//! - `hsm-pkcs11`: Generic HSM support via PKCS#11 interface

pub mod config;
pub mod device;
pub mod error;
pub mod pin;
pub mod signer;

#[cfg(feature = "hsm-ledger")]
pub mod ledger;

#[cfg(feature = "hsm-pkcs11")]
pub mod pkcs11;

// Re-exports
pub use config::HsmConfig;
pub use device::{HsmDeviceInfo, HsmDeviceType};
pub use error::{HsmError, HsmResult};
pub use pin::prompt_pin;
pub use signer::{HsmKeyInfo, HsmSigner};

#[cfg(feature = "hsm-ledger")]
pub use ledger::LedgerSigner;

#[cfg(feature = "hsm-pkcs11")]
pub use pkcs11::Pkcs11Signer;
