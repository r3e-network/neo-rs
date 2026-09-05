//! Wallets module for Neo blockchain
//!
//! This module provides wallet functionality matching the C# Neo.Wallets namespace.

/// Descriptors describing on-chain assets (C# `AssetDescriptor`).
pub mod asset_descriptor;
pub mod bip32;
pub mod bip39;
/// Helper utilities for wallet address, script hash, and verification script conversions.
pub mod helper;
pub mod key_pair;
pub mod nep6;
/// Transfer outputs describing asset movements from a wallet.
pub mod transfer_output;
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
