//! # neo-tee::wallet
//!
//! TEE wallet custody and signing helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-tee`. This adapter crate owns TEE integration
//! and must not define protocol bytes, consensus rules, or storage semantics.
//!
//! ## Contents
//!
//! - `provider`: Provider adapter for the surrounding trait boundary.
//! - `sealed_key`: sealed wallet key records.
//! - `tee_wallet`: TEE-backed wallet facade.

mod provider;
mod sealed_key;
mod tee_wallet;

pub use provider::TeeWalletProvider;
pub use sealed_key::SealedKey;
pub use tee_wallet::TeeWallet;
