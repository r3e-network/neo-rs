//! Wallet manager: factory registry and lifecycle operations.
//!
//! Mirrors the static factory methods on the C# `Wallet` class, grouping
//! create/open/migrate over a set of registered [`WalletFactory`]s.

use neo_config::ProtocolSettings;
use crate::wallet::{Wallet, WalletError, WalletResult};
use crate::wallet_factory::WalletFactory;

/// Static wallet factory methods.
/// This matches the static methods in the C# Wallet class.
pub struct WalletManager {
    factories: Vec<Box<dyn WalletFactory>>,
}

impl WalletManager {
    /// Creates a new wallet manager.
    pub fn new() -> Self {
        Self {
            factories: Vec::new(),
        }
    }

    /// Registers a wallet factory.
    pub fn register_factory(&mut self, factory: Box<dyn WalletFactory>) {
        self.factories.push(factory);
    }

    /// Creates a new wallet.
    pub async fn create_wallet(
        &self,
        name: &str,
        path: &str,
        password: &str,
        settings: &ProtocolSettings,
    ) -> WalletResult<Box<dyn Wallet>> {
        let factory = self
            .get_factory(path)
            .ok_or_else(|| WalletError::Other("No suitable factory found".to_string()))?;

        factory
            .create_wallet(name, path, password, settings)
            .map_err(WalletError::Other)
    }

    /// Opens an existing wallet.
    pub async fn open_wallet(
        &self,
        path: &str,
        password: &str,
        settings: &ProtocolSettings,
    ) -> WalletResult<Box<dyn Wallet>> {
        let factory = self
            .get_factory(path)
            .ok_or_else(|| WalletError::Other("No suitable factory found".to_string()))?;

        factory
            .open_wallet(path, password, settings)
            .map_err(WalletError::Other)
    }

    /// Migrates a wallet from one format to another.
    pub async fn migrate_wallet(
        &self,
        old_path: &str,
        new_path: &str,
        password: &str,
        settings: &ProtocolSettings,
    ) -> WalletResult<Box<dyn Wallet>> {
        let old_factory = self
            .get_factory(old_path)
            .ok_or_else(|| WalletError::Other("No suitable factory for old wallet".to_string()))?;

        let new_factory = self
            .get_factory(new_path)
            .ok_or_else(|| WalletError::Other("No suitable factory for new wallet".to_string()))?;

        // Open old wallet
        let old_wallet = old_factory
            .open_wallet(old_path, password, settings)
            .map_err(WalletError::Other)?;

        // Create new wallet
        let new_wallet = new_factory
            .create_wallet(old_wallet.name(), new_path, password, settings)
            .map_err(WalletError::Other)?;

        // Copy all accounts
        for account in old_wallet.get_accounts() {
            if let Some(key_pair) = account.get_key() {
                if let Some(contract) = account.contract() {
                    new_wallet
                        .create_account_with_contract(contract.clone(), Some(key_pair))
                        .await?;
                } else {
                    new_wallet.create_account(key_pair.private_key()).await?;
                }
            } else {
                // Watch-only account
                new_wallet
                    .create_account_watch_only(account.script_hash())
                    .await?;
            }
        }

        new_wallet.save().await?;
        Ok(new_wallet)
    }

    /// Gets the appropriate factory for the specified path.
    fn get_factory(&self, path: &str) -> Option<&dyn WalletFactory> {
        self.factories
            .iter()
            .find(|factory| factory.handle(path))
            .map(|factory| factory.as_ref())
    }
}

neo_io::impl_default_via_new!(WalletManager);
