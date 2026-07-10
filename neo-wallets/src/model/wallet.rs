//! Base wallet implementation.
//!
//! This module provides the base wallet trait and functionality,
//! converted from the C# Neo Wallet class (@neo-sharp/src/Neo/Wallets/Wallet.cs).

use crate::{key_pair::KeyPair, version::Version, wallet_account::WalletAccount};
use neo_payloads::Transaction;
use neo_primitives::{UInt160, UInt256};
use neo_vm::Contract;
use std::sync::Arc;

/// Result type for wallet operations
pub type WalletResult<T> = std::result::Result<T, WalletError>;

/// Wallet-specific errors
#[derive(thiserror::Error, Debug)]
pub enum WalletError {
    /// Password verification failed.
    #[error("Invalid password")]
    InvalidPassword,

    /// The requested account script hash is not present in the wallet.
    #[error("Account not found: {0}")]
    AccountNotFound(UInt160),

    /// The wallet file could not be found at the requested path.
    #[error("Wallet file not found: {0}")]
    WalletFileNotFound(String),

    /// The wallet file exists but does not match the expected format.
    #[error("Invalid wallet format")]
    InvalidWalletFormat,

    /// The wallet is locked and the requested operation requires unlocked keys.
    #[error("Wallet is locked")]
    WalletLocked,

    /// The selected account is locked and cannot sign or export keys.
    #[error("Account is locked")]
    AccountLocked,

    /// The wallet does not have enough spendable balance for the operation.
    #[error("Insufficient funds")]
    InsufficientFunds,

    /// Transaction construction failed.
    #[error("Transaction creation failed: {0}")]
    TransactionCreationFailed(String),

    /// Signing failed for the requested account or payload.
    #[error("Signing failed: {0}")]
    SigningFailed(String),

    /// Error propagated from shared Neo core functionality.
    #[error("Core error: {0}")]
    Core(#[from] neo_error::CoreError),

    /// Error propagated from filesystem or stream operations.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Miscellaneous wallet error that does not fit a more specific variant.
    #[error("Other error: {0}")]
    Other(String),
}

/// The base trait for all wallet implementations.
/// This matches the C# Wallet abstract class.
pub trait Wallet: Send + Sync {
    /// Concrete account representation returned by this wallet.
    type Account: WalletAccount + 'static;

    /// The name of the wallet.
    fn name(&self) -> &str;

    /// The path of the wallet file.
    fn path(&self) -> Option<&str>;

    /// The version of the wallet.
    fn version(&self) -> &Version;

    /// Changes the password of the wallet.
    fn change_password(
        &self,
        old_password: &str,
        new_password: &str,
    ) -> impl std::future::Future<Output = WalletResult<bool>> + Send;

    /// Checks whether the wallet contains the specified account.
    fn contains(&self, script_hash: &UInt160) -> bool;

    /// Creates a new account with the specified private key.
    fn create_account(
        &self,
        private_key: &[u8],
    ) -> impl std::future::Future<Output = WalletResult<Arc<Self::Account>>> + Send;

    /// Creates a new account with the specified contract and key pair.
    fn create_account_with_contract(
        &self,
        contract: Contract,
        key_pair: Option<KeyPair>,
    ) -> impl std::future::Future<Output = WalletResult<Arc<Self::Account>>> + Send;

    /// Creates a new account with the specified script hash (watch-only).
    fn create_account_watch_only(
        &self,
        script_hash: UInt160,
    ) -> impl std::future::Future<Output = WalletResult<Arc<Self::Account>>> + Send;

    /// Deletes the specified account from the wallet.
    fn delete_account(
        &self,
        script_hash: &UInt160,
    ) -> impl std::future::Future<Output = WalletResult<bool>> + Send;

    /// Exports the wallet to the specified path.
    fn export(
        &self,
        path: &str,
        password: &str,
    ) -> impl std::future::Future<Output = WalletResult<()>> + Send;

    /// Gets the account with the specified script hash.
    fn account(&self, script_hash: &UInt160) -> Option<Arc<Self::Account>>;

    /// Gets all accounts in the wallet.
    fn accounts(&self) -> Vec<Arc<Self::Account>>;

    /// Gets the available balance of the specified asset.
    fn available_balance(
        &self,
        asset_id: &UInt256,
    ) -> impl std::future::Future<Output = WalletResult<i64>> + Send;

    /// Gets the unclaimed GAS amount.
    fn unclaimed_gas(&self) -> impl std::future::Future<Output = WalletResult<i64>> + Send;

    /// Imports an account from a WIF (Wallet Import Format) private key.
    fn import_wif(
        &self,
        wif: &str,
    ) -> impl std::future::Future<Output = WalletResult<Arc<Self::Account>>> + Send;

    /// Imports an account from a NEP-2 encrypted private key.
    fn import_nep2(
        &self,
        nep2_key: &str,
        password: &str,
    ) -> impl std::future::Future<Output = WalletResult<Arc<Self::Account>>> + Send;

    /// Signs the specified data with the specified account.
    fn sign(
        &self,
        data: &[u8],
        script_hash: &UInt160,
    ) -> impl std::future::Future<Output = WalletResult<Vec<u8>>> + Send;

    /// Signs the specified transaction.
    fn sign_transaction(
        &self,
        transaction: &mut Transaction,
    ) -> impl std::future::Future<Output = WalletResult<()>> + Send;

    /// Unlocks the wallet with the specified password.
    fn unlock(
        &self,
        password: &str,
    ) -> impl std::future::Future<Output = WalletResult<bool>> + Send;

    /// Locks the wallet.
    fn lock(&self);

    /// Checks whether the specified password is correct.
    fn verify_password(
        &self,
        password: &str,
    ) -> impl std::future::Future<Output = WalletResult<bool>> + Send;

    /// Saves the wallet to disk.
    fn save(&self) -> impl std::future::Future<Output = WalletResult<()>> + Send;

    /// Gets the default account.
    fn default_account(&self) -> Option<Arc<Self::Account>>;

    /// Sets the default account.
    fn set_default_account(
        &self,
        script_hash: &UInt160,
    ) -> impl std::future::Future<Output = WalletResult<()>> + Send;
}
