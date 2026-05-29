//! Wallets module for Neo blockchain
//!
//! This module provides wallet functionality matching the C# Neo.Wallets namespace.

pub mod asset_descriptor;
pub mod bip32;
pub mod bip39;
pub mod helper;
pub mod wallet_factory;
pub mod wallet_provider;
pub mod key_pair;
pub mod nep6;
pub mod scripts;
pub mod transfer_output;
pub mod version;
pub mod wallet;
pub mod wallet_account;

// Re-export commonly used types
pub use asset_descriptor::AssetDescriptor;
pub use bip32::{ExtendedKey, KeyPath};
pub use bip39::{get_mnemonic_code, get_mnemonic_code_with_language, mnemonic_to_entropy};
pub use helper::Helper;
pub use wallet_factory::WalletFactory;
pub use wallet_provider::WalletProvider;
pub use key_pair::KeyPair;
pub use nep6::{Nep6Account, Nep6Wallet, ScryptParameters};
pub use scripts::signature_invocation;
pub use transfer_output::TransferOutput;
pub use version::Version;
pub use wallet::{Wallet, WalletError, WalletManager, WalletResult};
pub use wallet_account::{StandardWalletAccount, WalletAccount};
