//! Base wallet implementation.
//!
//! This module provides the base wallet trait and functionality,
//! converted from the C# Neo Wallet class (@neo-sharp/src/Neo/Wallets/Wallet.cs).

use neo_execution::Contract;
use crate::{key_pair::KeyPair, version::Version, wallet_account::WalletAccount};
use neo_payloads::Transaction;
use neo_primitives::{UInt160, UInt256};
use async_trait::async_trait;
use std::sync::Arc;

/// Result type for wallet operations
pub type WalletResult<T> = std::result::Result<T, WalletError>;

/// Wallet-specific errors
#[derive(thiserror::Error, Debug)]
pub enum WalletError {
    #[error("Invalid password")]
    InvalidPassword,

    #[error("Account not found: {0}")]
    AccountNotFound(UInt160),

    #[error("Wallet file not found: {0}")]
    WalletFileNotFound(String),

    #[error("Invalid wallet format")]
    InvalidWalletFormat,

    #[error("Wallet is locked")]
    WalletLocked,

    #[error("Account is locked")]
    AccountLocked,

    #[error("Insufficient funds")]
    InsufficientFunds,

    #[error("Transaction creation failed: {0}")]
    TransactionCreationFailed(String),

    #[error("Signing failed: {0}")]
    SigningFailed(String),

    #[error("Core error: {0}")]
    Core(#[from] neo_error::CoreError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Other error: {0}")]
    Other(String),
}

/// The base trait for all wallet implementations.
/// This matches the C# Wallet abstract class.
#[async_trait]
pub trait Wallet: Send + Sync {
    /// The name of the wallet.
    fn name(&self) -> &str;

    /// The path of the wallet file.
    fn path(&self) -> Option<&str>;

    /// The version of the wallet.
    fn version(&self) -> &Version;

    /// Changes the password of the wallet.
    async fn change_password(&self, old_password: &str, new_password: &str) -> WalletResult<bool>;

    /// Checks whether the wallet contains the specified account.
    fn contains(&self, script_hash: &UInt160) -> bool;

    /// Creates a new account with the specified private key.
    async fn create_account(&self, private_key: &[u8]) -> WalletResult<Arc<dyn WalletAccount>>;

    /// Creates a new account with the specified contract and key pair.
    async fn create_account_with_contract(
        &self,
        contract: Contract,
        key_pair: Option<KeyPair>,
    ) -> WalletResult<Arc<dyn WalletAccount>>;

    /// Creates a new account with the specified script hash (watch-only).
    async fn create_account_watch_only(
        &self,
        script_hash: UInt160,
    ) -> WalletResult<Arc<dyn WalletAccount>>;

    /// Deletes the specified account from the wallet.
    async fn delete_account(&self, script_hash: &UInt160) -> WalletResult<bool>;

    /// Exports the wallet to the specified path.
    async fn export(&self, path: &str, password: &str) -> WalletResult<()>;

    /// Gets the account with the specified script hash.
    fn get_account(&self, script_hash: &UInt160) -> Option<Arc<dyn WalletAccount>>;

    /// Gets all accounts in the wallet.
    fn get_accounts(&self) -> Vec<Arc<dyn WalletAccount>>;

    /// Gets the available balance of the specified asset.
    async fn get_available_balance(&self, asset_id: &UInt256) -> WalletResult<i64>;

    /// Gets the unclaimed GAS amount.
    async fn get_unclaimed_gas(&self) -> WalletResult<i64>;

    /// Imports an account from a WIF (Wallet Import Format) private key.
    async fn import_wif(&self, wif: &str) -> WalletResult<Arc<dyn WalletAccount>>;

    /// Imports an account from a NEP-2 encrypted private key.
    async fn import_nep2(
        &self,
        nep2_key: &str,
        password: &str,
    ) -> WalletResult<Arc<dyn WalletAccount>>;

    /// Signs the specified data with the specified account.
    async fn sign(&self, data: &[u8], script_hash: &UInt160) -> WalletResult<Vec<u8>>;

    /// Signs the specified transaction.
    async fn sign_transaction(&self, transaction: &mut Transaction) -> WalletResult<()>;

    /// Unlocks the wallet with the specified password.
    async fn unlock(&self, password: &str) -> WalletResult<bool>;

    /// Locks the wallet.
    fn lock(&self);

    /// Checks whether the specified password is correct.
    async fn verify_password(&self, password: &str) -> WalletResult<bool>;

    /// Saves the wallet to disk.
    async fn save(&self) -> WalletResult<()>;

    /// Gets the default account.
    fn get_default_account(&self) -> Option<Arc<dyn WalletAccount>>;

    /// Sets the default account.
    async fn set_default_account(&self, script_hash: &UInt160) -> WalletResult<()>;
}

