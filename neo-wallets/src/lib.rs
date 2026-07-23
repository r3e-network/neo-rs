// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! # neo-wallets
//!
//! Wallet models, key derivation, accounts, scripts, and transfer helpers.
//!
//! ## Boundary
//!
//! This wallet crate owns account and signing helpers and must not import
//! blocks, run services, or mutate node storage directly.
//!
//! ## Contents
//!
//! - `model`: wallet model records, NEP-6 files, and account helpers.
//! - `assets`: Wallet asset descriptors and transfer output records.
//! - `bip32`: BIP-32 derivation helpers for wallet keys.
//! - `crypto`: Wallet key pairs, signing, and address helpers.
//! - `scripting`: Wallet script construction and verification helpers.

#![doc(html_root_url = "https://docs.rs/neo-wallets/0.11.1")]

// ============================================================================
// Wallet data model
// ============================================================================

pub mod model;

// ============================================================================
// Wallet crypto + scripting
// ============================================================================

pub mod assets;
/// BIP-32 extended keys and derivation paths.
pub mod bip32;
pub mod crypto;
pub mod scripting;

pub use assets::{asset_descriptor, transfer_output};
pub use crypto::{bip39, key_pair};
pub use model::{nep6, version, wallet, wallet_account, wallet_helper, wallet_provider};
pub use scripting::scripts;

// ============================================================================
// Public re-exports
// ============================================================================

pub use asset_descriptor::AssetDescriptor;
pub use bip32::{ExtendedKey, KeyPath};
pub use bip39::Bip39;
pub use key_pair::KeyPair;
pub use nep6::{Nep6Account, Nep6Wallet, ScryptParameters};
pub use scripts::signature_invocation;
pub use transfer_output::TransferOutput;
pub use version::Version;
pub use wallet::{Wallet, WalletError, WalletResult};
pub use wallet_account::{StandardWalletAccount, WalletAccount};

pub use wallet_provider::WalletProvider;
