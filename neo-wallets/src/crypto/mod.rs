//! # neo-wallets::crypto
//!
//! Wallet key pairs, signing, and address helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-wallets`. This wallet crate owns account and
//! signing helpers and must not import blocks, run services, or mutate node
//! storage directly.
//!
//! ## Contents
//!
//! - `bip39`: BIP-39 mnemonic helpers.
//! - `key_pair`: wallet key-pair records and signing helpers.

pub mod bip39;
pub mod key_pair;
