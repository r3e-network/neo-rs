//! SQLite Wallet implementation
//!
//! Provides SQLite-based wallet functionality for Neo blockchain.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use rusqlite::{Connection, Result as SqliteResult, params};

use super::{
    sq_lite_wallet_account::SQLiteWalletAccount,
    address::Address,
    key::Key,
    contract::Contract,
    verification_contract::VerificationContract,
    wallet_data_context::WalletDataContext,
};

/// SQLite Wallet implementation
pub struct SQLiteWallet {
    /// Database connection
    pub connection: Arc<RwLock<Connection>>,
    /// Wallet file path
    pub wallet_path: PathBuf,
    /// Wallet name
    pub name: String,
    /// Wallet version
    pub version: String,
    /// Is wallet open
    pub is_open: bool,
    /// Wallet accounts
    pub accounts: Arc<RwLock<Vec<SQLiteWalletAccount>>>,
}

impl SQLiteWallet {
    /// Create a new SQLite wallet
    pub fn new(wallet_path: PathBuf, name: String) -> Self {
        Self {
            connection: Arc::new(RwLock::new(Connection::open_in_memory().unwrap())),
            wallet_path,
            name,
            version: "1.0.0".to_string(),
            is_open: false,
            accounts: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Open the wallet
    pub async fn open(&mut self) -> Result<(), String> {
        if self.is_open {
            return Ok(());
        }

        // Open SQLite database
        let conn = Connection::open(&self.wallet_path)
            .map_err(|e| format!("Failed to open wallet database: {}", e))?;

        // Create tables if they don't exist
        self.create_tables(&conn).await?;

        // Load accounts
        self.load_accounts(&conn).await?;

        self.connection = Arc::new(RwLock::new(conn));
        self.is_open = true;

        Ok(())
    }

    /// Close the wallet
    pub async fn close(&mut self) -> Result<(), String> {
        if !self.is_open {
            return Ok(());
        }

        // Save accounts
        self.save_accounts().await?;

        self.is_open = false;
        Ok(())
    }

    /// Create database tables
    async fn create_tables(&self, conn: &Connection) -> Result<(), String> {
        // Create accounts table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS accounts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                address TEXT NOT NULL UNIQUE,
                label TEXT,
                is_default INTEGER NOT NULL DEFAULT 0,
                lock BOOLEAN NOT NULL DEFAULT 0,
                contract TEXT,
                created_at INTEGER NOT NULL
            )",
            [],
        ).map_err(|e| format!("Failed to create accounts table: {}", e))?;

        // Create keys table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS keys (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                account_id INTEGER NOT NULL,
                private_key TEXT NOT NULL,
                public_key TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                FOREIGN KEY (account_id) REFERENCES accounts (id)
            )",
            [],
        ).map_err(|e| format!("Failed to create keys table: {}", e))?;

        // Create contracts table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS contracts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                account_id INTEGER NOT NULL,
                script TEXT NOT NULL,
                parameter_list TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                FOREIGN KEY (account_id) REFERENCES accounts (id)
            )",
            [],
        ).map_err(|e| format!("Failed to create contracts table: {}", e))?;

        Ok(())
    }

    /// Load accounts from database
    async fn load_accounts(&self, conn: &Connection) -> Result<(), String> {
        let mut stmt = conn.prepare(
            "SELECT id, address, label, is_default, lock, contract, created_at FROM accounts"
        ).map_err(|e| format!("Failed to prepare accounts query: {}", e))?;

        let account_iter = stmt.query_map([], |row| {
            Ok(SQLiteWalletAccount {
                id: row.get(0)?,
                address: Address::from_string(row.get::<_, String>(1)?)?,
                label: row.get(2)?,
                is_default: row.get::<_, i32>(3)? != 0,
                lock: row.get::<_, i32>(4)? != 0,
                contract: row.get(5)?,
                created_at: row.get(6)?,
            })
        }).map_err(|e| format!("Failed to query accounts: {}", e))?;

        let mut accounts = Vec::new();
        for account_result in account_iter {
            let account = account_result.map_err(|e| format!("Failed to parse account: {}", e))?;
            accounts.push(account);
        }

        let mut accounts_guard = self.accounts.write().await;
        *accounts_guard = accounts;

        Ok(())
    }

    /// Save accounts to database
    async fn save_accounts(&self) -> Result<(), String> {
        let conn = self.connection.read().await;
        let accounts = self.accounts.read().await;

        for account in accounts.iter() {
            conn.execute(
                "INSERT OR REPLACE INTO accounts (id, address, label, is_default, lock, contract, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    account.id,
                    account.address.to_string(),
                    account.label,
                    if account.is_default { 1 } else { 0 },
                    if account.lock { 1 } else { 0 },
                    account.contract,
                    account.created_at
                ],
            ).map_err(|e| format!("Failed to save account: {}", e))?;
        }

        Ok(())
    }

    /// Create a new account
    pub async fn create_account(&self, label: Option<String>) -> Result<SQLiteWalletAccount, String> {
        if !self.is_open {
            return Err("Wallet is not open".to_string());
        }

        // Generate new key pair
        let key = Key::generate()?;
        let address = Address::from_public_key(&key.public_key)?;

        let account = SQLiteWalletAccount {
            id: 0, // Will be set by database
            address,
            label: label.unwrap_or_else(|| "Default".to_string()),
            is_default: false,
            lock: false,
            contract: None,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        // Save to database
        let conn = self.connection.read().await;
        conn.execute(
            "INSERT INTO accounts (address, label, is_default, lock, contract, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                account.address.to_string(),
                account.label,
                if account.is_default { 1 } else { 0 },
                if account.lock { 1 } else { 0 },
                account.contract,
                account.created_at
            ],
        ).map_err(|e| format!("Failed to create account: {}", e))?;

        // Add to in-memory list
        let mut accounts = self.accounts.write().await;
        accounts.push(account.clone());

        Ok(account)
    }

    /// Get account by address
    pub async fn get_account(&self, address: &Address) -> Result<Option<SQLiteWalletAccount>, String> {
        let accounts = self.accounts.read().await;
        Ok(accounts.iter().find(|a| &a.address == address).cloned())
    }

    /// Get all accounts
    pub async fn get_accounts(&self) -> Result<Vec<SQLiteWalletAccount>, String> {
        let accounts = self.accounts.read().await;
        Ok(accounts.clone())
    }

    /// Delete an account
    pub async fn delete_account(&self, address: &Address) -> Result<(), String> {
        if !self.is_open {
            return Err("Wallet is not open".to_string());
        }

        // Remove from database
        let conn = self.connection.read().await;
        conn.execute(
            "DELETE FROM accounts WHERE address = ?1",
            params![address.to_string()],
        ).map_err(|e| format!("Failed to delete account: {}", e))?;

        // Remove from in-memory list
        let mut accounts = self.accounts.write().await;
        accounts.retain(|a| &a.address != address);

        Ok(())
    }

    /// Set default account
    pub async fn set_default_account(&self, address: &Address) -> Result<(), String> {
        if !self.is_open {
            return Err("Wallet is not open".to_string());
        }

        // Update database
        let conn = self.connection.read().await;
        
        // Clear all default flags
        conn.execute("UPDATE accounts SET is_default = 0", [])
            .map_err(|e| format!("Failed to clear default flags: {}", e))?;

        // Set new default
        conn.execute(
            "UPDATE accounts SET is_default = 1 WHERE address = ?1",
            params![address.to_string()],
        ).map_err(|e| format!("Failed to set default account: {}", e))?;

        // Update in-memory list
        let mut accounts = self.accounts.write().await;
        for account in accounts.iter_mut() {
            account.is_default = &account.address == address;
        }

        Ok(())
    }

    /// Get wallet information
    pub fn get_wallet_info(&self) -> (String, String, bool) {
        (self.name.clone(), self.version.clone(), self.is_open)
    }
}
