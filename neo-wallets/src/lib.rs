// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! # neo-wallets
//!
//! Canonical home for the Neo wallet layer: keypair, BIP-32/BIP-39 helpers,
//! NEP-6 wallets, wallet accounts, the `Wallet` trait, the wallet manager and
//! factory registry, plus the witness-script helpers used by all wallet
//! implementations.
//!
//! Mirrors `Neo.Wallets` for the wallet data model and the helpers any
//! concrete wallet (software, HSM, TEE) needs. The stateful runtime
//! (transaction signing against the ledger, RPC client, fee calculation
//! against `PolicyContract`) lives in `neo-core`; this crate stays
//! pure-data and pure-crypto so it can be embedded in wallets that do
//! not want the full node runtime.
//!
//! ## Layering
//!
//! Sits in **Layer 1 (protocol)**. Depends on:
//!
//! - `neo-primitives`, `neo-error`, `neo-io`, `neo-crypto` (Layer 0)
//! - `neo-config`, `neo-script-builder`, `neo-storage`,
//!   `neo-payloads`, `neo-execution`,
//!   `neo-manifest` (Layer 1)
//!
//! Must **not** depend on `neo-core` (Layer 2 runtime) or any Layer 2+
//! crate. This matches the rule polkadot-sdk and reth apply to their
//! `*-wallets` crates: keep the wallet data model and signing
//! primitives independent of the node runtime that consumes them.

#![doc(html_root_url = "https://docs.rs/neo-wallets/0.7.2")]
#![deny(unsafe_code)]
#![warn(missing_docs)]
#![allow(dead_code)]
#![allow(unused_imports)]

// ============================================================================
// Wallet data model
// ============================================================================

/// Wallet account abstraction and the standard in-memory implementation.
pub mod wallet_account;

/// NEP-6 wallet standard.
pub mod nep6;
/// Base `Wallet` trait and shared error type.
pub mod wallet;
/// Wallet factory trait.
pub mod wallet_factory;
/// Address / script-hash conversion helpers used by the wallet layer.
pub mod wallet_helper;
/// Wallet manager (factory registry + lifecycle).
pub mod wallet_manager;
/// Wallet provider (lifecycle notifications).
pub mod wallet_provider;

// ============================================================================
// Wallet crypto + scripting
// ============================================================================

/// NEP-17 asset descriptor (name / symbol / decimals lookup).
pub mod asset_descriptor;
/// BIP-32 extended keys and derivation paths.
pub mod bip32;
/// BIP-39 mnemonics.
pub mod bip39;
/// ECDSA key pair (private + public key, encryption round-trips).
pub mod key_pair;
/// Witness-script helpers (signature invocation script, etc.).
pub mod scripts;
/// NEP-17 transfer output descriptor.
pub mod transfer_output;
/// Three-component wallet `Version`.
pub mod version;

// ============================================================================
// Public re-exports
// ============================================================================

pub use asset_descriptor::AssetDescriptor;
pub use bip32::{ExtendedKey, KeyPath};
pub use bip39::{get_mnemonic_code, get_mnemonic_code_with_language, mnemonic_to_entropy};
pub use key_pair::KeyPair;
pub use nep6::{Nep6Account, Nep6Wallet, ScryptParameters};
pub use scripts::signature_invocation;
pub use transfer_output::TransferOutput;
pub use version::Version;
pub use wallet::{Wallet, WalletError, WalletResult};
pub use wallet_account::{StandardWalletAccount, WalletAccount};
pub use wallet_factory::WalletFactory;

// Re-export of the canonical smart-contract Helper for back-compat with the
// historical `neo_wallets::Helper` (which was a type alias for
// `crate::smart_contract::helper::Helper`).
pub use neo_execution::Helper;
pub use wallet_manager::WalletManager;
pub use wallet_provider::WalletProvider;
