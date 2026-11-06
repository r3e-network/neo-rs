//! Base wallet abstraction, error type, version, and manager.
//!
//! Ported from `neo-core`'s `wallets::wallet` and `wallets::version`. The trait
//! is made async and focused on key/account management: balance queries,
//! transfer construction and transaction signing require the ledger and
//! smart-contract engine (forbidden dependencies here) and are therefore left as
//! task 89 alignment hooks rather than ported.

use crate::account::WalletAccount;
use crate::key_pair::KeyPair;
use async_trait::async_trait;
use neo_primitives::{UInt160, UInt256};
use neo_protocol::ProtocolSettings;
use neo_smart_contract::contract::Contract;
use std::fmt;
use std::sync::Arc;

/// Result type for wallet operations.
pub type WalletResult<T> = std::result::Result<T, WalletError>;

/// Wallet-specific errors.
#[derive(thiserror::Error, Debug)]
pub enum WalletError {
    /// The supplied password is incorrect.
    #[error("Invalid password")]
    InvalidPassword,

    /// The requested account does not exist in the wallet.
    #[error("Account not found: {0}")]
    AccountNotFound(UInt160),

    /// No wallet file exists at the given path.
    #[error("Wallet file not found: {0}")]
    WalletFileNotFound(String),

    /// The wallet file is not a valid NEP-6 document.
    #[error("Invalid wallet format")]
    InvalidWalletFormat,

    /// The wallet is locked and must be unlocked first.
    #[error("Wallet is locked")]
    WalletLocked,

    /// The account is locked and its key is unavailable.
    #[error("Account is locked")]
    AccountLocked,

    /// The wallet lacks sufficient balance for the operation.
    #[error("Insufficient funds")]
    InsufficientFunds,

    /// Building the transaction failed.
    #[error("Transaction creation failed: {0}")]
    TransactionCreationFailed(String),

    /// Signing the data failed.
    #[error("Signing failed: {0}")]
    SigningFailed(String),

    /// A cryptographic primitive (ECC / hashing / point) operation failed.
    #[error("Crypto error: {0}")]
    Crypto(String),

    /// A NEP-2 encrypted key is malformed.
    #[error("Invalid NEP-2 key: {0}")]
    InvalidNep2Key(String),

    /// A WIF string is malformed.
    #[error("Invalid WIF: {0}")]
    InvalidWif(String),

    /// A private key is malformed.
    #[error("Invalid private key: {0}")]
    InvalidPrivateKey(String),

    /// scrypt key derivation failed.
    #[error("Scrypt error: {0}")]
    Scrypt(String),

    /// AES encryption/decryption failed.
    #[error("AES error: {0}")]
    Aes(String),

    /// An I/O error occurred during a wallet operation.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Any other wallet error.
    #[error("Other error: {0}")]
    Other(String),
}

/// A semantic version triple used by wallet file formats (C# `Version`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    /// Major component.
    pub major: u8,
    /// Minor component.
    pub minor: u8,
    /// Build/patch component.
    pub build: u8,
}

impl Version {
    /// Creates a version from its three components.
    #[must_use]
    pub const fn new(major: u8, minor: u8, build: u8) -> Self {
        Self {
            major,
            minor,
            build,
        }
    }

    /// The NEP-6 wallet format version (`1.0.0`).
    #[must_use]
    pub const fn nep6() -> Self {
        Self::new(1, 0, 0)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.build)
    }
}

impl Default for Version {
    fn default() -> Self {
        Self::nep6()
    }
}

/// The base trait for all wallet implementations (C# `Wallet` abstract class).
///
/// Synchronous and key-management focused. Ledger-backed operations (balances,
/// unclaimed GAS, transfer/transaction building and signing) are intentionally
/// omitted; see the task 89 alignment hooks in the crate handoff report.
pub trait Wallet: Send + Sync {
    /// The name of the wallet.
    fn name(&self) -> &str;

    /// The path of the wallet file.
    fn path(&self) -> Option<&str>;

    /// The version of the wallet.
    fn version(&self) -> &Version;

    /// Checks whether the wallet contains the specified account.
    fn contains(&self, script_hash: &UInt160) -> bool;

    /// Creates a new account with the specified private key.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError`] when the key is invalid or the account exists.
    fn create_account(&self, private_key: &[u8]) -> WalletResult<Arc<dyn WalletAccount>>;

    /// Creates a new account with the specified contract and optional key pair.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError`] when the account cannot be created.
    fn create_account_with_contract(
        &self,
        contract: Contract,
        key_pair: Option<KeyPair>,
    ) -> WalletResult<Arc<dyn WalletAccount>>;

    /// Creates a new watch-only account for the specified script hash.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError`] when the account cannot be created.
    fn create_account_watch_only(
        &self,
        script_hash: UInt160,
    ) -> WalletResult<Arc<dyn WalletAccount>>;

    /// Deletes the specified account from the wallet.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError`] when the account cannot be removed.
    fn delete_account(&self, script_hash: &UInt160) -> WalletResult<bool>;

    /// Gets the account with the specified script hash.
    fn get_account(&self, script_hash: &UInt160) -> Option<Arc<dyn WalletAccount>>;

    /// Gets all accounts in the wallet.
    fn get_accounts(&self) -> Vec<Arc<dyn WalletAccount>>;

    /// Gets the default account.
    fn get_default_account(&self) -> Option<Arc<dyn WalletAccount>>;

    /// Sets the default account.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::AccountNotFound`] when the account is missing.
    fn set_default_account(&self, script_hash: &UInt160) -> WalletResult<()>;

    /// Imports an account from a WIF (Wallet Import Format) private key.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::InvalidWif`] for malformed input.
    fn import_wif(&self, wif: &str) -> WalletResult<Arc<dyn WalletAccount>>;

    /// Imports an account from a NEP-2 encrypted private key.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::InvalidNep2Key`] or [`WalletError::InvalidPassword`].
    fn import_nep2(&self, nep2_key: &str, password: &str) -> WalletResult<Arc<dyn WalletAccount>>;

    /// Exports the wallet to the specified path.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::Io`] when the file cannot be written.
    fn export(&self, path: &str, password: &str) -> WalletResult<()>;

    /// Changes the password protecting every encrypted key in the wallet.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::InvalidPassword`] when the old password is wrong.
    fn change_password(&self, old_password: &str, new_password: &str) -> WalletResult<bool>;

    /// Signs the supplied preimage with the specified account's key.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::AccountLocked`] or [`WalletError::SigningFailed`].
    fn sign(&self, data: &[u8], script_hash: &UInt160) -> WalletResult<Vec<u8>>;

    /// Unlocks every account with the supplied password.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::InvalidPassword`] when the password is wrong.
    fn unlock(&self, password: &str) -> WalletResult<bool>;

    /// Locks the wallet.
    fn lock(&self);

    /// Checks whether the specified password is correct.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError`] when verification encounters a fatal error.
    fn verify_password(&self, password: &str) -> WalletResult<bool>;

    /// Saves the wallet to disk.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::Io`] when the file cannot be written.
    fn save(&self) -> WalletResult<()>;
}

/// Factory that can create/open a concrete wallet format for a path.
pub trait WalletFactory: Send + Sync {
    /// Returns whether this factory handles the given path.
    fn handle(&self, path: &str) -> bool;

    /// Creates a new wallet at the path.
    ///
    /// # Errors
    ///
    /// Returns a string error when creation fails.
    fn create_wallet(
        &self,
        name: &str,
        path: &str,
        password: &str,
        settings: &Arc<ProtocolSettings>,
    ) -> Result<Box<dyn Wallet>, String>;

    /// Opens an existing wallet at the path.
    ///
    /// # Errors
    ///
    /// Returns a string error when opening fails.
    fn open_wallet(
        &self,
        path: &str,
        password: &str,
        settings: &Arc<ProtocolSettings>,
    ) -> Result<Box<dyn Wallet>, String>;
}

/// Static wallet factory methods (C# `Wallet` static methods).
pub struct WalletManager {
    factories: Vec<Box<dyn WalletFactory>>,
}

impl WalletManager {
    /// Creates a new wallet manager with no registered factories.
    #[must_use]
    pub fn new() -> Self {
        Self {
            factories: Vec::new(),
        }
    }

    /// Registers a wallet factory.
    pub fn register_factory(&mut self, factory: Box<dyn WalletFactory>) {
        self.factories.push(factory);
    }

    /// Creates a new wallet using the factory that handles the path.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::Other`] when no suitable factory is registered.
    pub fn create_wallet(
        &self,
        name: &str,
        path: &str,
        password: &str,
        settings: &Arc<ProtocolSettings>,
    ) -> WalletResult<Box<dyn Wallet>> {
        let factory = self
            .get_factory(path)
            .ok_or_else(|| WalletError::Other("No suitable factory found".to_string()))?;
        factory
            .create_wallet(name, path, password, settings)
            .map_err(WalletError::Other)
    }

    /// Opens an existing wallet using the factory that handles the path.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::Other`] when no suitable factory is registered.
    pub fn open_wallet(
        &self,
        path: &str,
        password: &str,
        settings: &Arc<ProtocolSettings>,
    ) -> WalletResult<Box<dyn Wallet>> {
        let factory = self
            .get_factory(path)
            .ok_or_else(|| WalletError::Other("No suitable factory found".to_string()))?;
        factory
            .open_wallet(path, password, settings)
            .map_err(WalletError::Other)
    }

    /// Migrates a wallet from one format/path to another, copying all accounts.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError`] when either wallet cannot be opened/created or an
    /// account cannot be copied.
    pub fn migrate_wallet(
        &self,
        old_path: &str,
        new_path: &str,
        password: &str,
        settings: &Arc<ProtocolSettings>,
    ) -> WalletResult<Box<dyn Wallet>> {
        let old_factory = self
            .get_factory(old_path)
            .ok_or_else(|| WalletError::Other("No suitable factory for old wallet".to_string()))?;
        let new_factory = self
            .get_factory(new_path)
            .ok_or_else(|| WalletError::Other("No suitable factory for new wallet".to_string()))?;

        let old_wallet = old_factory
            .open_wallet(old_path, password, settings)
            .map_err(WalletError::Other)?;
        let new_wallet = new_factory
            .create_wallet(old_wallet.name(), new_path, password, settings)
            .map_err(WalletError::Other)?;

        for account in old_wallet.get_accounts() {
            if let Some(key_pair) = account.get_key() {
                if let Some(contract) = account.contract() {
                    new_wallet.create_account_with_contract(contract.clone(), Some(key_pair))?;
                } else {
                    new_wallet.create_account(key_pair.private_key())?;
                }
            } else {
                new_wallet.create_account_watch_only(account.script_hash())?;
            }
        }

        new_wallet.save()?;
        Ok(new_wallet)
    }

    /// Gets the appropriate factory for the specified path.
    fn get_factory(&self, path: &str) -> Option<&dyn WalletFactory> {
        self.factories
            .iter()
            .find(|factory| factory.handle(path))
            .map(|factory| &**factory)
    }
}

impl Default for WalletManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Marker re-export so downstream integrators can reference the asset id type
/// without pulling `neo-primitives` directly (kept for API parity with neo-core).
pub type AssetId = UInt256;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_display_and_defaults() {
        assert_eq!(Version::new(1, 0, 0).to_string(), "1.0.0");
        assert_eq!(Version::default(), Version::nep6());
    }

    #[test]
    fn wallet_manager_without_factory_reports_other() {
        let manager = WalletManager::new();
        let settings = Arc::new(ProtocolSettings::default_settings());
        // `open_wallet` returns `Result<Box<dyn Wallet>, _>`; the `Ok` type is
        // not `Debug`, so match on the result instead of using `unwrap_err`.
        let result = manager.open_wallet("missing.json", "pw", &settings);
        assert!(matches!(result, Err(WalletError::Other(_))));
    }
}
