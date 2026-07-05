//! # neo-hsm
//!
//! Hardware security module configuration and signing-provider adapters.
//!
//! ## Boundary
//!
//! This adapter crate owns signing-provider integration and must not own
//! consensus, ledger persistence, or node orchestration.
//!
//! ## Contents
//!
//! - `errors`: Typed errors and result aliases for this crate boundary.
//! - `providers`: Provider implementations behind the crate public traits.
//! - `settings`: HSM provider settings and signing profile records.

mod errors;
mod providers;
mod settings;

// ── Convenient re-exports ────────────────────────────────────────────────────

pub use errors::{HsmError, HsmResult, error};
pub use settings::{HsmConfig, HsmProvider, ProviderProfile, SigFormat, config, profile};

#[cfg(feature = "pkcs11")]
pub use providers::{Pkcs11Signer, pkcs11};

#[cfg(feature = "azure")]
pub use providers::{AzureKeyVaultConfig, AzureKeyVaultSigner, azure};

#[cfg(feature = "gcp")]
pub use providers::{GcpKmsConfig, GcpKmsSigner, gcp};
