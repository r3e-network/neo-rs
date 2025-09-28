//! Wallets module for Neo blockchain
//!
//! This module provides wallet functionality matching the C# Neo.Wallets namespace.

pub mod asset_descriptor;
pub mod helper;
pub mod i_wallet_factory;
pub mod i_wallet_provider;
pub mod key_pair;
pub mod nep6;
pub mod transfer_output;
pub mod wallet;
pub mod wallet_account;

// Re-export commonly used types
pub use asset_descriptor::AssetDescriptor;
pub use helper::Helper;
pub use i_wallet_factory::IWalletFactory;
pub use i_wallet_provider::IWalletProvider;
pub use key_pair::KeyPair;
pub use transfer_output::TransferOutput;
pub use wallet::{Wallet, WalletError, WalletManager, WalletResult};
pub use wallet_account::{BaseWalletAccount, WalletAccount};
