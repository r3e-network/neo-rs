//! # neo-crypto::keys
//!
//! Wallet-facing key derivation, signing, and verification helpers.
//!
//! The actual curve and signature engines come from upstream crates; this
//! module keeps the Neo-facing glue: BIP-32 scalar addition, curve selection,
//! raw byte shapes, and C# parity around error mapping.
//!
//! ## Boundary
//!
//! This module belongs to `neo-crypto`. This foundation crate owns
//! cryptographic primitives and must not depend on node services, RPC, storage
//! engines, or UI crates.
//!
//! ## Contents
//!
//! - `bip32`: low-level BIP-32 primitives for wallet derivation.
//! - `signature`: signature adapters and verification helpers.

pub mod bip32;
pub mod signature;
