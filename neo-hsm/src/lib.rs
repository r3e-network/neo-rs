//! # neo-hsm
//!
//! Cloud-HSM-backed validator key management for the Neo N3 consensus node.
//!
//! ## Overview
//!
//! This crate provides implementations of [`neo_consensus::ConsensusSigner`]
//! that keep the validator private key inside a hardware security module,
//! never exposing it to the host process.  Three cloud providers are
//! supported:
//!
//! | Provider            | Path           | Feature   | Sig format   |
//! |---------------------|----------------|-----------|--------------|
//! | AWS CloudHSM        | PKCS#11        | `pkcs11`  | raw `r‖s`    |
//! | Azure Cloud HSM     | PKCS#11        | `pkcs11`  | raw `r‖s`    |
//! | Azure Dedicated HSM | PKCS#11        | `pkcs11`  | raw `r‖s`    |
//! | GCP Cloud KMS       | PKCS#11 (kmsp11)| `pkcs11` | DER → `r‖s`  |
//! | Azure Managed HSM   | REST (native)  | `azure`   | raw `r‖s`    |
//! | GCP Cloud KMS       | REST (native)  | `gcp`     | DER → `r‖s`  |
//!
//! ## Cryptographic contract
//!
//! Neo N3 consensus requires a **64-byte raw `r‖s` secp256r1 ECDSA**
//! signature over `SHA-256(data)`.  All signers in this crate:
//!
//! 1. Hash the input with `Crypto::sha256` before calling the HSM (`CKM_ECDSA`
//!    signs a pre-hashed digest, not raw data).
//! 2. Decode DER if necessary (GCP paths only).
//! 3. Apply low-s normalization (`Signature::normalize_s`) for C# parity.
//!
//! ## `!Send` / `!Sync` session confinement
//!
//! The PKCS#11 `Session` is `Send` but `!Sync`.  [`Pkcs11Signer`] confines
//! the session and the `Pkcs11` context to a single dedicated worker thread
//! reached via an `mpsc::Sender`.  The public struct is `Send + Sync` with
//! zero `unsafe` code.
//!
//! ## Usage (PKCS#11 path)
//!
//! ```rust,no_run
//! # #[cfg(feature = "pkcs11")]
//! # {
//! use neo_hsm::{HsmConfig, HsmProvider, Pkcs11Signer};
//! use std::path::PathBuf;
//!
//! let cfg = HsmConfig {
//!     provider: HsmProvider::Aws,
//!     library_path: PathBuf::from("/opt/cloudhsm/lib/libcloudhsm_pkcs11.so"),
//!     slot: Some(0),
//!     token_label: None,
//!     key_label: "neo-validator-1".to_string(),
//!     key_id: None,
//!     user_pin: "CryptoUser:s3cr3t".to_string(), // load from env in production
//! };
//! let signer = Pkcs11Signer::connect(&cfg).expect("HSM connect failed");
//! println!("validator pubkey: {}", hex::encode(signer.public_key()));
//! # }
//! ```

#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod config;
pub mod error;

#[cfg(feature = "pkcs11")]
pub mod pkcs11;

#[cfg(feature = "azure")]
pub mod azure;

#[cfg(feature = "gcp")]
pub mod gcp;

// ── Convenient re-exports ────────────────────────────────────────────────────

pub use config::{HsmConfig, HsmProvider, ProviderProfile, SigFormat, profile};
pub use error::{HsmError, HsmResult};

#[cfg(feature = "pkcs11")]
pub use pkcs11::Pkcs11Signer;

#[cfg(feature = "azure")]
pub use azure::{AzureKeyVaultConfig, AzureKeyVaultSigner};

#[cfg(feature = "gcp")]
pub use gcp::{GcpKmsConfig, GcpKmsSigner};
