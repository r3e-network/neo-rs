//! # neo-wallets::bip32
//!
//! BIP-32 derivation helpers for wallet keys.
//!
//! ## Boundary
//!
//! This module belongs to `neo-wallets`. This wallet crate owns account and
//! signing helpers and must not import blocks, run services, or mutate node
//! storage directly.
//!
//! ## Contents
//!
//! - `extended_key`: extended private/public key records.
//! - `key_path`: BIP-32 key path parser and formatter.

pub mod extended_key;
pub mod key_path;

pub use extended_key::ExtendedKey;
pub use key_path::KeyPath;
