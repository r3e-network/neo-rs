//! SQLite Wallet Account implementation
//!
//! Provides account functionality for SQLite wallet.

use super::address::Address;
use serde::{Deserialize, Serialize};

/// SQLite Wallet Account structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SQLiteWalletAccount {
    /// Account ID
    pub id: i64,
    /// Account address
    pub address: Address,
    /// Account label
    pub label: String,
    /// Is default account
    pub is_default: bool,
    /// Is account locked
    pub lock: bool,
    /// Account contract
    pub contract: Option<String>,
    /// Created timestamp
    pub created_at: u64,
}

impl SQLiteWalletAccount {
    /// Create a new wallet account
    pub fn new(
        id: i64,
        address: Address,
        label: String,
        is_default: bool,
        lock: bool,
        contract: Option<String>,
    ) -> Self {
        Self {
            id,
            address,
            label,
            is_default,
            lock,
            contract,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }

    /// Get account address
    pub fn get_address(&self) -> &Address {
        &self.address
    }

    /// Get account label
    pub fn get_label(&self) -> &str {
        &self.label
    }

    /// Set account label
    pub fn set_label(&mut self, label: String) {
        self.label = label;
    }

    /// Check if account is default
    pub fn is_default(&self) -> bool {
        self.is_default
    }

    /// Set default status
    pub fn set_default(&mut self, is_default: bool) {
        self.is_default = is_default;
    }

    /// Check if account is locked
    pub fn is_locked(&self) -> bool {
        self.lock
    }

    /// Set lock status
    pub fn set_lock(&mut self, lock: bool) {
        self.lock = lock;
    }

    /// Get account contract
    pub fn get_contract(&self) -> Option<&str> {
        self.contract.as_deref()
    }

    /// Set account contract
    pub fn set_contract(&mut self, contract: Option<String>) {
        self.contract = contract;
    }

    /// Get account ID
    pub fn get_id(&self) -> i64 {
        self.id
    }

    /// Get created timestamp
    pub fn get_created_at(&self) -> u64 {
        self.created_at
    }

    /// Check if account is valid
    pub fn is_valid(&self) -> bool {
        !self.address.to_string().is_empty() && !self.label.is_empty()
    }
}
