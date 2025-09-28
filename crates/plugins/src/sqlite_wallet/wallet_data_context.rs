//! Wallet Data Context implementation
//!
//! Provides wallet data context functionality for Neo blockchain.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::{
    sq_lite_wallet_account::SQLiteWalletAccount,
    address::Address,
    key::Key,
    contract::Contract,
    verification_contract::VerificationContract,
};

/// Wallet Data Context structure
#[derive(Debug, Clone)]
pub struct WalletDataContext {
    /// Wallet accounts
    pub accounts: Arc<RwLock<HashMap<String, SQLiteWalletAccount>>>,
    /// Wallet keys
    pub keys: Arc<RwLock<HashMap<String, Key>>>,
    /// Wallet contracts
    pub contracts: Arc<RwLock<HashMap<String, Contract>>>,
    /// Wallet verification contracts
    pub verification_contracts: Arc<RwLock<HashMap<String, VerificationContract>>>,
}

impl WalletDataContext {
    /// Create a new wallet data context
    pub fn new() -> Self {
        Self {
            accounts: Arc::new(RwLock::new(HashMap::new())),
            keys: Arc::new(RwLock::new(HashMap::new())),
            contracts: Arc::new(RwLock::new(HashMap::new())),
            verification_contracts: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add an account
    pub async fn add_account(&self, account: SQLiteWalletAccount) -> Result<(), String> {
        let mut accounts = self.accounts.write().await;
        accounts.insert(account.address.to_string(), account);
        Ok(())
    }

    /// Get an account by address
    pub async fn get_account(&self, address: &Address) -> Result<Option<SQLiteWalletAccount>, String> {
        let accounts = self.accounts.read().await;
        Ok(accounts.get(&address.to_string()).cloned())
    }

    /// Remove an account
    pub async fn remove_account(&self, address: &Address) -> Result<(), String> {
        let mut accounts = self.accounts.write().await;
        accounts.remove(&address.to_string());
        Ok(())
    }

    /// Get all accounts
    pub async fn get_all_accounts(&self) -> Result<Vec<SQLiteWalletAccount>, String> {
        let accounts = self.accounts.read().await;
        Ok(accounts.values().cloned().collect())
    }

    /// Add a key
    pub async fn add_key(&self, address: &Address, key: Key) -> Result<(), String> {
        let mut keys = self.keys.write().await;
        keys.insert(address.to_string(), key);
        Ok(())
    }

    /// Get a key by address
    pub async fn get_key(&self, address: &Address) -> Result<Option<Key>, String> {
        let keys = self.keys.read().await;
        Ok(keys.get(&address.to_string()).cloned())
    }

    /// Remove a key
    pub async fn remove_key(&self, address: &Address) -> Result<(), String> {
        let mut keys = self.keys.write().await;
        keys.remove(&address.to_string());
        Ok(())
    }

    /// Add a contract
    pub async fn add_contract(&self, address: &Address, contract: Contract) -> Result<(), String> {
        let mut contracts = self.contracts.write().await;
        contracts.insert(address.to_string(), contract);
        Ok(())
    }

    /// Get a contract by address
    pub async fn get_contract(&self, address: &Address) -> Result<Option<Contract>, String> {
        let contracts = self.contracts.read().await;
        Ok(contracts.get(&address.to_string()).cloned())
    }

    /// Remove a contract
    pub async fn remove_contract(&self, address: &Address) -> Result<(), String> {
        let mut contracts = self.contracts.write().await;
        contracts.remove(&address.to_string());
        Ok(())
    }

    /// Add a verification contract
    pub async fn add_verification_contract(&self, address: &Address, contract: VerificationContract) -> Result<(), String> {
        let mut verification_contracts = self.verification_contracts.write().await;
        verification_contracts.insert(address.to_string(), contract);
        Ok(())
    }

    /// Get a verification contract by address
    pub async fn get_verification_contract(&self, address: &Address) -> Result<Option<VerificationContract>, String> {
        let verification_contracts = self.verification_contracts.read().await;
        Ok(verification_contracts.get(&address.to_string()).cloned())
    }

    /// Remove a verification contract
    pub async fn remove_verification_contract(&self, address: &Address) -> Result<(), String> {
        let mut verification_contracts = self.verification_contracts.write().await;
        verification_contracts.remove(&address.to_string());
        Ok(())
    }

    /// Get wallet statistics
    pub async fn get_wallet_stats(&self) -> (usize, usize, usize, usize) {
        let accounts_count = self.accounts.read().await.len();
        let keys_count = self.keys.read().await.len();
        let contracts_count = self.contracts.read().await.len();
        let verification_contracts_count = self.verification_contracts.read().await.len();
        (accounts_count, keys_count, contracts_count, verification_contracts_count)
    }

    /// Clear all data
    pub async fn clear_all(&self) -> Result<(), String> {
        let mut accounts = self.accounts.write().await;
        accounts.clear();

        let mut keys = self.keys.write().await;
        keys.clear();

        let mut contracts = self.contracts.write().await;
        contracts.clear();

        let mut verification_contracts = self.verification_contracts.write().await;
        verification_contracts.clear();

        Ok(())
    }
}
