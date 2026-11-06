//! Wallets module for Neo blockchain
//!
//! This module provides wallet functionality matching the C# Neo.Wallets namespace.
//!
//! # Relationship to the `neo-wallets` crate (Task 198)
//!
//! `neo-wallets` is a *lower* layer: `neo-core` depends on it (see
//! `neo-core/Cargo.toml`), so it cannot depend back on `neo-core`. The truly
//! decoupled leaf modules ([`bip32`], [`bip39`], [`transfer_output`]) were
//! folded into `neo-wallets` by Task 173 and are re-exported below.
//!
//! The remaining modules stay in `neo-core` on purpose: they are the
//! neo-core-*integrated* wallet runtime and are bound to higher-layer engine
//! types that `neo-wallets` cannot reference. Collapsing them into a
//! `neo-wallets` re-export shim would be a layering violation and would not
//! compile:
//!
//! * [`wallet`] — the `Wallet` trait is `async` and carries ledger-backed
//!   methods (`get_available_balance`, `get_unclaimed_gas`,
//!   `sign_transaction(&mut Transaction)`); it is implemented by
//!   `neo-node`'s `HsmWallet` and held as `Arc<dyn Wallet>` in
//!   `signing_provider`/`consensus`. `neo-wallets`' `Wallet` is synchronous
//!   and intentionally omits those methods.
//! * [`helper`] / [`asset_descriptor`] — depend on `ApplicationEngine`,
//!   `DataCache`, `ScriptBuilder`, the native contracts and `Verifiable`.
//! * [`wallet_account`] / [`nep6`] — depend on `Transaction`/`Witness`/
//!   `get_sign_data_vec` and are `async`.
//! * [`key_pair`] — returns `CoreResult`/`CoreError` (the `neo-wallets`
//!   `KeyPair` returns `WalletResult`/`WalletError`); the two variants serve
//!   different layers and consumers.
//! * [`version`] — divergent representation (`u32` fields + getters +
//!   `FromStr`) versus the `neo-wallets` `Version` (`u8` public fields).

/// Descriptors describing on-chain assets (C# `AssetDescriptor`).
pub mod asset_descriptor;
// Task 173 (B2): folded into the authoritative `neo-wallets` crate. These leaf
// modules have byte-identical public APIs (no coupling to neo-core's async
// `Wallet` trait or `CoreError`), so they are re-exported rather than duplicated.
pub use neo_wallets::bip32;
pub use neo_wallets::bip39;
/// Helper utilities for wallet address, script hash, and verification script conversions.
pub mod helper;
pub mod key_pair;
pub mod nep6;
// Task 173 (B2): folded into `neo-wallets` (identical struct + fields).
/// Transfer outputs describing asset movements from a wallet.
pub use neo_wallets::transfer_output;
pub mod version;
pub mod wallet;
pub mod wallet_account;
/// Factory for creating wallet instances from files or in memory.
pub mod wallet_factory;
/// Provider that tracks the currently active wallet for the node.
pub mod wallet_provider;

// Re-export commonly used types
pub use asset_descriptor::AssetDescriptor;
pub use bip32::{ExtendedKey, KeyPath};
pub use bip39::{get_mnemonic_code, get_mnemonic_code_with_language, mnemonic_to_entropy};
pub use helper::Helper;
pub use key_pair::KeyPair;
pub use nep6::{Nep6Account, Nep6Wallet, ScryptParameters};
pub use transfer_output::TransferOutput;
pub use version::Version;
pub use wallet::{Wallet, WalletError, WalletManager, WalletResult};
pub use wallet_account::{StandardWalletAccount, WalletAccount};
pub use wallet_factory::WalletFactory;
pub use wallet_provider::WalletProvider;
