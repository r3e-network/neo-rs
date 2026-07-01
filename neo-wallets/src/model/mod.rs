//! # neo-wallets::model
//!
//! Wallet model records, NEP-6 files, accounts, and provider traits.
//!
//! ## Boundary
//!
//! This module belongs to `neo-wallets`. This wallet crate owns account and
//! signing helpers and must not import blocks, run services, or mutate node
//! storage directly.
//!
//! ## Contents
//!
//! - `nep6`: NEP-6 wallet model records.
//! - `version`: wallet version records.
//! - `wallet`: Wallet trait, account storage, and signing facade.
//! - `wallet_account`: wallet account records and encryption helpers.
//! - `wallet_helper`: wallet helper functions.
//! - `wallet_provider`: wallet provider adapter.

/// NEP-6 wallet standard.
pub mod nep6;
/// Three-component wallet `Version`.
pub mod version;
/// Base `Wallet` trait and shared error type.
pub mod wallet;
/// Wallet account abstraction and the standard in-memory implementation.
pub mod wallet_account;
/// Address / script-hash conversion helpers used by the wallet layer.
pub mod wallet_helper;
/// Wallet provider lifecycle notifications.
pub mod wallet_provider;
