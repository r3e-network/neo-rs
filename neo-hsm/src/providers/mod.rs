//! # neo-hsm::providers
//!
//! Provider implementations behind the crate public traits.
//!
//! ## Boundary
//!
//! This module belongs to `neo-hsm`. This adapter crate owns signing-provider
//! integration and must not own consensus, ledger persistence, or node
//! orchestration.
//!
//! ## Contents
//!
//! - `azure`: Azure Key Vault HSM provider.
//! - `gcp`: Google Cloud KMS provider.
//! - `pkcs11`: PKCS#11 HSM provider.

#[cfg(feature = "azure")]
pub mod azure;

#[cfg(feature = "gcp")]
pub mod gcp;

#[cfg(feature = "pkcs11")]
pub mod pkcs11;

#[cfg(feature = "azure")]
pub use azure::{AzureKeyVaultConfig, AzureKeyVaultSigner};

#[cfg(feature = "gcp")]
pub use gcp::{GcpKmsConfig, GcpKmsSigner};

#[cfg(feature = "pkcs11")]
pub use pkcs11::Pkcs11Signer;
