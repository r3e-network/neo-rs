//! # neo-crypto::keys
//!
//! Key derivation, signing, and verification helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-crypto`. This foundation crate owns
//! cryptographic primitives and must not depend on node services, RPC, storage
//! engines, or UI crates.
//!
//! ## Contents
//!
//! - `bip32`: BIP-32 derivation helpers for wallet keys.
//! - `signature`: signature records and verification helpers.

pub mod bip32;
pub mod signature;
