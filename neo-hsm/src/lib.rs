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
//!
//! Phase 3 S6b: sank from neo-node (hsm tree + hsm_integration runtime +
//! hsm_wallet). All modules stay behind the opt-in `hsm` feature, mirroring
//! the node's historical gating so default builds remain hardware-free.

#[cfg(feature = "hsm")]
pub mod config;
// Phase 9 (F15): injectable credential acquisition (replaces hardcoded stdin).
#[cfg(feature = "hsm")]
pub mod credentials;
#[cfg(feature = "hsm")]
pub mod device;
#[cfg(feature = "hsm")]
pub mod error;
#[cfg(feature = "hsm")]
pub mod pin;
#[cfg(feature = "hsm")]
pub mod signer;

#[cfg(feature = "hsm")]
mod runtime;
#[cfg(feature = "hsm")]
mod wallet;

#[cfg(feature = "hsm-ledger")]
pub mod ledger;

#[cfg(feature = "hsm-pkcs11")]
pub mod pkcs11;

// Re-exports
#[cfg(feature = "hsm")]
pub use config::HsmConfig;
#[cfg(feature = "hsm")]
pub use credentials::{ConsoleCredentialProvider, CredentialProvider};
#[cfg(feature = "hsm")]
pub use device::{HsmDeviceInfo, HsmDeviceType};
#[cfg(feature = "hsm")]
pub use error::{HsmError, HsmResult};
#[cfg(feature = "hsm")]
pub use pin::prompt_pin;
#[cfg(feature = "hsm")]
pub use runtime::{
    HsmRuntime, initialize_with_config, initialize_with_config_interactive, print_hsm_status,
    run_hardware_operation,
};
#[cfg(feature = "hsm")]
pub use signer::{HsmKeyInfo, HsmSigner};
#[cfg(feature = "hsm")]
pub use wallet::HsmWallet;

#[cfg(feature = "hsm-ledger")]
pub use ledger::LedgerSigner;

#[cfg(feature = "hsm-pkcs11")]
pub use pkcs11::Pkcs11Signer;
