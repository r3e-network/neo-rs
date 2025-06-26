//! SQLiteWallet Plugin
//!
//! This plugin provides SQLite database backend support for Neo wallets,
//! allowing storage and management of wallet data in SQLite database files.

use crate::{Plugin, PluginCategory, PluginContext, PluginEvent, PluginInfo};
use async_trait::async_trait;
use neo_core::{UInt160, UInt256};
use neo_extensions::error::{ExtensionError, ExtensionResult};
use rusqlite::{Connection, Result as SqliteResult, params};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// SQLite wallet account record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqliteWalletAccount {
    /// Account address
    pub address: String,
    /// Account label
    pub label: String,
    /// Whether this is a default account
    pub is_default: bool,
    /// Lock status
    pub lock: bool,
    /// Encrypted private key
    pub key: Option<String>,
    /// Contract script hash
    pub contract: Option<String>,
    /// Extra data
    pub extra: Option<String>,
}

/// SQLite wallet contract record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqliteWalletContract {
    /// Contract script hash
    pub script_hash: String,
    /// Contract script
    pub script: String,
    /// Contract parameters
    pub parameters: String,
    /// Whether contract is deployed
    pub deployed: bool,
}

/// SQLite wallet address record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqliteWalletAddress {
    /// Address string
    pub address: String,
    /// Associated script hash
    pub script_hash: String,
}

/// SQLite wallet key record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqliteWalletKey {
    /// Public key
    pub public_key: String,
    /// Encrypted private key
    pub private_key: String,
    /// Associated account
    pub account: String,
}

/// SQLite wallet coin record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqliteWalletCoin {
    /// Transaction hash
    pub tx_hash: String,
    /// Output index
    pub output_index: u32,
    /// Asset ID
    pub asset_id: String,
    /// Value
    pub value: String,
    /// Script hash
    pub script_hash: String,
    /// State (unspent/spent)
    pub state: String,
}

/// SQLite wallet transaction record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqliteWalletTransaction {
    /// Transaction hash
    pub hash: String,
    /// Transaction height
    pub height: u32,
    /// Transaction time
    pub time: u64,
    /// Transaction data
    pub data: String,
}

/// SQLite wallet implementation
pub struct SqliteWallet {
    /// Database file path
    pub db_path: PathBuf,
    /// Database connection (wrapped for async safety)
    connection: Arc<RwLock<Option<Connection>>>,
    /// Wallet version
    pub version: String,
    /// Wallet name
    pub name: String,
    /// Extra data
    pub extra: HashMap<String, String>,
}

impl SqliteWallet {
    /// Create a new SQLite wallet
    pub fn new(db_path: PathBuf) -> Self {
        Self {
            db_path,
            connection: Arc::new(RwLock::new(None)),
            version: "3.0".to_string(),
            name: "SQLite Wallet".to_string(),
            extra: HashMap::new(),
        }
    }

    /// Open or create the SQLite database
    pub async fn open(&mut self) -> ExtensionResult<()> {
        let conn = Connection::open(&self.db_path)
            .map_err(|e| ExtensionError::DatabaseError(e.to_string()))?;

        // Create tables if they don't exist
        self.create_tables(&conn)?;

        let mut connection = self.connection.write().await;
        *connection = Some(conn);

        info!("SQLite wallet opened: {:?}", self.db_path);
        Ok(())
    }

    /// Create database tables
    fn create_tables(&self, conn: &Connection) -> ExtensionResult<()> {
        // Account table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS Account (
                PublicKeyHash BLOB PRIMARY KEY,
                Label TEXT,
                IsDefault INTEGER,
                Lock INTEGER,
                Key BLOB,
                Contract BLOB,
                Extra TEXT
            )",
            [],
        )
        .map_err(|e| ExtensionError::DatabaseError(e.to_string()))?;

        // Address table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS Address (
                ScriptHash BLOB PRIMARY KEY,
                Address TEXT
            )",
            [],
        )
        .map_err(|e| ExtensionError::DatabaseError(e.to_string()))?;

        // Contract table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS Contract (
                ScriptHash BLOB PRIMARY KEY,
                Script BLOB,
                ParameterList BLOB,
                Deployed INTEGER
            )",
            [],
        )
        .map_err(|e| ExtensionError::DatabaseError(e.to_string()))?;

        // Key table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS Key (
                PublicKey BLOB PRIMARY KEY,
                PrivateKey BLOB,
                Account BLOB
            )",
            [],
        )
        .map_err(|e| ExtensionError::DatabaseError(e.to_string()))?;

        // Coin table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS Coin (
                TxId BLOB,
                OutputIndex INTEGER,
                AssetId BLOB,
                Value BLOB,
                ScriptHash BLOB,
                State INTEGER,
                PRIMARY KEY (TxId, OutputIndex)
            )",
            [],
        )
        .map_err(|e| ExtensionError::DatabaseError(e.to_string()))?;

        // Transaction table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS Transaction (
                Hash BLOB PRIMARY KEY,
                Height INTEGER,
                Time INTEGER,
                Data BLOB
            )",
            [],
        )
        .map_err(|e| ExtensionError::DatabaseError(e.to_string()))?;

        debug!("SQLite wallet tables created/verified");
        Ok(())
    }

    /// Close the database connection
    pub async fn close(&mut self) -> ExtensionResult<()> {
        let mut connection = self.connection.write().await;
        *connection = None;
        info!("SQLite wallet closed");
        Ok(())
    }

    /// Add an account to the wallet
    pub async fn add_account(&self, account: &SqliteWalletAccount) -> ExtensionResult<()> {
        let connection = self.connection.read().await;
        if let Some(conn) = connection.as_ref() {
            let address_bytes = hex::decode(&account.address.replace("0x", ""))
                .map_err(|e| ExtensionError::ValidationError(e.to_string()))?;

            conn.execute(
                "INSERT OR REPLACE INTO Account (PublicKeyHash, Label, IsDefault, Lock, Key, Contract, Extra)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    address_bytes,
                    account.label,
                    account.is_default as i32,
                    account.lock as i32,
                    account.key.as_deref(),
                    account.contract.as_deref(),
                    account.extra.as_deref()
                ],
            ).map_err(|e| ExtensionError::DatabaseError(e.to_string()))?;

            debug!("Added account: {}", account.address);
            Ok(())
        } else {
            Err(ExtensionError::NotInitialized)
        }
    }

    /// Get an account by address
    pub async fn get_account(&self, address: &str) -> ExtensionResult<Option<SqliteWalletAccount>> {
        let connection = self.connection.read().await;
        if let Some(conn) = connection.as_ref() {
            let address_bytes = hex::decode(&address.replace("0x", ""))
                .map_err(|e| ExtensionError::ValidationError(e.to_string()))?;

            let mut stmt = conn
                .prepare(
                    "SELECT PublicKeyHash, Label, IsDefault, Lock, Key, Contract, Extra 
                 FROM Account WHERE PublicKeyHash = ?1",
                )
                .map_err(|e| ExtensionError::DatabaseError(e.to_string()))?;

            let account_result = stmt.query_row(params![address_bytes], |row| {
                let hash_bytes: Vec<u8> = row.get(0)?;
                let hash_hex = format!("0x{}", hex::encode(hash_bytes));

                Ok(SqliteWalletAccount {
                    address: hash_hex,
                    label: row.get(1)?,
                    is_default: row.get::<_, i32>(2)? != 0,
                    lock: row.get::<_, i32>(3)? != 0,
                    key: row.get(4)?,
                    contract: row.get(5)?,
                    extra: row.get(6)?,
                })
            });

            match account_result {
                Ok(account) => Ok(Some(account)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(ExtensionError::DatabaseError(e.to_string())),
            }
        } else {
            Err(ExtensionError::NotInitialized)
        }
    }

    /// Get all accounts
    pub async fn get_all_accounts(&self) -> ExtensionResult<Vec<SqliteWalletAccount>> {
        let connection = self.connection.read().await;
        if let Some(conn) = connection.as_ref() {
            let mut stmt = conn.prepare(
                "SELECT PublicKeyHash, Label, IsDefault, Lock, Key, Contract, Extra FROM Account"
            ).map_err(|e| ExtensionError::DatabaseError(e.to_string()))?;

            let account_iter = stmt
                .query_map([], |row| {
                    let hash_bytes: Vec<u8> = row.get(0)?;
                    let hash_hex = format!("0x{}", hex::encode(hash_bytes));

                    Ok(SqliteWalletAccount {
                        address: hash_hex,
                        label: row.get(1)?,
                        is_default: row.get::<_, i32>(2)? != 0,
                        lock: row.get::<_, i32>(3)? != 0,
                        key: row.get(4)?,
                        contract: row.get(5)?,
                        extra: row.get(6)?,
                    })
                })
                .map_err(|e| ExtensionError::DatabaseError(e.to_string()))?;

            let mut accounts = Vec::new();
            for account_result in account_iter {
                accounts.push(
                    account_result.map_err(|e| ExtensionError::DatabaseError(e.to_string()))?,
                );
            }

            Ok(accounts)
        } else {
            Err(ExtensionError::NotInitialized)
        }
    }

    /// Add a contract to the wallet
    pub async fn add_contract(&self, contract: &SqliteWalletContract) -> ExtensionResult<()> {
        let connection = self.connection.read().await;
        if let Some(conn) = connection.as_ref() {
            let script_hash_bytes = hex::decode(&contract.script_hash.replace("0x", ""))
                .map_err(|e| ExtensionError::ValidationError(e.to_string()))?;
            let script_bytes = hex::decode(&contract.script.replace("0x", ""))
                .map_err(|e| ExtensionError::ValidationError(e.to_string()))?;

            conn.execute(
                "INSERT OR REPLACE INTO Contract (ScriptHash, Script, ParameterList, Deployed)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    script_hash_bytes,
                    script_bytes,
                    contract.parameters.as_bytes(),
                    contract.deployed as i32
                ],
            )
            .map_err(|e| ExtensionError::DatabaseError(e.to_string()))?;

            debug!("Added contract: {}", contract.script_hash);
            Ok(())
        } else {
            Err(ExtensionError::NotInitialized)
        }
    }

    /// Add a transaction to the wallet
    pub async fn add_transaction(
        &self,
        transaction: &SqliteWalletTransaction,
    ) -> ExtensionResult<()> {
        let connection = self.connection.read().await;
        if let Some(conn) = connection.as_ref() {
            let hash_bytes = hex::decode(&transaction.hash.replace("0x", ""))
                .map_err(|e| ExtensionError::ValidationError(e.to_string()))?;
            let data_bytes = hex::decode(&transaction.data.replace("0x", ""))
                .map_err(|e| ExtensionError::ValidationError(e.to_string()))?;

            conn.execute(
                "INSERT OR REPLACE INTO Transaction (Hash, Height, Time, Data)
                 VALUES (?1, ?2, ?3, ?4)",
                params![hash_bytes, transaction.height, transaction.time, data_bytes],
            )
            .map_err(|e| ExtensionError::DatabaseError(e.to_string()))?;

            debug!("Added transaction: {}", transaction.hash);
            Ok(())
        } else {
            Err(ExtensionError::NotInitialized)
        }
    }

    /// Delete an account
    pub async fn delete_account(&self, address: &str) -> ExtensionResult<bool> {
        let connection = self.connection.read().await;
        if let Some(conn) = connection.as_ref() {
            let address_bytes = hex::decode(&address.replace("0x", ""))
                .map_err(|e| ExtensionError::ValidationError(e.to_string()))?;

            let rows_affected = conn
                .execute(
                    "DELETE FROM Account WHERE PublicKeyHash = ?1",
                    params![address_bytes],
                )
                .map_err(|e| ExtensionError::DatabaseError(e.to_string()))?;

            debug!(
                "Deleted account: {} (rows affected: {})",
                address, rows_affected
            );
            Ok(rows_affected > 0)
        } else {
            Err(ExtensionError::NotInitialized)
        }
    }

    /// Get wallet statistics
    pub async fn get_statistics(&self) -> ExtensionResult<HashMap<String, u64>> {
        let connection = self.connection.read().await;
        if let Some(conn) = connection.as_ref() {
            let mut stats = HashMap::new();

            // Count accounts
            let account_count: u64 = conn
                .query_row("SELECT COUNT(*) FROM Account", [], |row| row.get(0))
                .unwrap_or(0);
            stats.insert("accounts".to_string(), account_count);

            // Count contracts
            let contract_count: u64 = conn
                .query_row("SELECT COUNT(*) FROM Contract", [], |row| row.get(0))
                .unwrap_or(0);
            stats.insert("contracts".to_string(), contract_count);

            // Count transactions
            let transaction_count: u64 = conn
                .query_row("SELECT COUNT(*) FROM Transaction", [], |row| row.get(0))
                .unwrap_or(0);
            stats.insert("transactions".to_string(), transaction_count);

            // Count coins
            let coin_count: u64 = conn
                .query_row("SELECT COUNT(*) FROM Coin", [], |row| row.get(0))
                .unwrap_or(0);
            stats.insert("coins".to_string(), coin_count);

            Ok(stats)
        } else {
            Err(ExtensionError::NotInitialized)
        }
    }

    /// Check if wallet file is a valid SQLite wallet
    pub fn is_sqlite_wallet(file_path: &Path) -> bool {
        if !file_path.exists() {
            return false;
        }

        // Check file extension
        if let Some(extension) = file_path.extension() {
            if extension != "db3" {
                return false;
            }
        } else {
            return false;
        }

        // Try to open as SQLite database and check for wallet tables
        if let Ok(conn) = Connection::open(file_path) {
            let tables_exist = conn
                .execute(
                    "SELECT name FROM sqlite_master WHERE type='table' AND name='Account'",
                    [],
                )
                .is_ok();

            tables_exist
        } else {
            false
        }
    }
}

/// SQLiteWallet plugin implementation
pub struct SqliteWalletPlugin {
    info: PluginInfo,
    enabled: bool,
    wallets: Arc<RwLock<HashMap<String, SqliteWallet>>>,
    default_wallet_path: Option<PathBuf>,
}

impl SqliteWalletPlugin {
    /// Create a new SQLiteWallet plugin
    pub fn new() -> Self {
        Self {
            info: PluginInfo {
                name: "SQLiteWallet".to_string(),
                version: "3.6.0".to_string(),
                description: "Provides SQLite database backend for Neo wallets".to_string(),
                author: "Neo Rust Team".to_string(),
                dependencies: vec![],
                min_neo_version: "3.0.0".to_string(),
                category: PluginCategory::Wallet,
                priority: 60,
            },
            enabled: true,
            wallets: Arc::new(RwLock::new(HashMap::new())),
            default_wallet_path: None,
        }
    }

    /// Open a SQLite wallet
    pub async fn open_wallet(&self, file_path: PathBuf) -> ExtensionResult<String> {
        if !SqliteWallet::is_sqlite_wallet(&file_path) {
            return Err(ExtensionError::ValidationError(
                "File is not a valid SQLite wallet".to_string(),
            ));
        }

        let wallet_id = file_path.to_string_lossy().to_string();
        let mut wallet = SqliteWallet::new(file_path);
        wallet.open().await?;

        let mut wallets = self.wallets.write().await;
        wallets.insert(wallet_id.clone(), wallet);

        info!("Opened SQLite wallet: {}", wallet_id);
        Ok(wallet_id)
    }

    /// Create a new SQLite wallet
    pub async fn create_wallet(&self, file_path: PathBuf, name: &str) -> ExtensionResult<String> {
        let wallet_id = file_path.to_string_lossy().to_string();
        let mut wallet = SqliteWallet::new(file_path);
        wallet.name = name.to_string();
        wallet.open().await?;

        let mut wallets = self.wallets.write().await;
        wallets.insert(wallet_id.clone(), wallet);

        info!("Created SQLite wallet: {} ({})", name, wallet_id);
        Ok(wallet_id)
    }

    /// Close a wallet
    pub async fn close_wallet(&self, wallet_id: &str) -> ExtensionResult<()> {
        let mut wallets = self.wallets.write().await;
        if let Some(mut wallet) = wallets.remove(wallet_id) {
            wallet.close().await?;
            info!("Closed SQLite wallet: {}", wallet_id);
        }
        Ok(())
    }

    /// Get wallet by ID
    pub async fn get_wallet(&self, wallet_id: &str) -> Option<SqliteWallet> {
        let wallets = self.wallets.read().await;
        wallets.get(wallet_id).cloned()
    }

    /// List all open wallets
    pub async fn list_wallets(&self) -> Vec<String> {
        let wallets = self.wallets.read().await;
        wallets.keys().cloned().collect()
    }
}

impl Default for SqliteWalletPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for SqliteWalletPlugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }

    async fn initialize(&mut self, context: &PluginContext) -> ExtensionResult<()> {
        info!("Initializing SQLiteWallet plugin");

        // Load configuration
        let config_file = context.config_dir.join("SQLiteWallet.json");
        if config_file.exists() {
            match tokio::fs::read_to_string(&config_file).await {
                Ok(config_str) => {
                    if let Ok(config) = serde_json::from_str::<serde_json::Value>(&config_str) {
                        if let Some(enabled) = config.get("enabled").and_then(|v| v.as_bool()) {
                            self.enabled = enabled;
                        }

                        if let Some(default_path) =
                            config.get("default_wallet_path").and_then(|v| v.as_str())
                        {
                            self.default_wallet_path = Some(PathBuf::from(default_path));
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to read SQLiteWallet config: {}", e);
                }
            }
        }

        info!(
            "SQLiteWallet plugin initialized (enabled: {})",
            self.enabled
        );
        Ok(())
    }

    async fn start(&mut self) -> ExtensionResult<()> {
        if !self.enabled {
            info!("SQLiteWallet plugin is disabled");
            return Ok(());
        }

        info!("Starting SQLiteWallet plugin");

        // Open default wallet if configured
        if let Some(default_path) = &self.default_wallet_path {
            if default_path.exists() {
                match self.open_wallet(default_path.clone()).await {
                    Ok(wallet_id) => {
                        info!("Opened default SQLite wallet: {}", wallet_id);
                    }
                    Err(e) => {
                        warn!("Failed to open default SQLite wallet: {}", e);
                    }
                }
            }
        }

        info!("SQLiteWallet plugin started successfully");
        Ok(())
    }

    async fn stop(&mut self) -> ExtensionResult<()> {
        info!("Stopping SQLiteWallet plugin");

        // Close all open wallets
        let wallet_ids = self.list_wallets().await;
        for wallet_id in wallet_ids {
            if let Err(e) = self.close_wallet(&wallet_id).await {
                warn!("Error closing wallet {}: {}", wallet_id, e);
            }
        }

        info!("SQLiteWallet plugin stopped");
        Ok(())
    }

    async fn handle_event(&mut self, event: &PluginEvent) -> ExtensionResult<()> {
        if !self.enabled {
            return Ok(());
        }

        match event {
            PluginEvent::Custom { event_type, data } => match event_type.as_str() {
                "open_wallet" => {
                    if let Some(path) = data.get("path").and_then(|v| v.as_str()) {
                        let _ = self.open_wallet(PathBuf::from(path)).await;
                    }
                }
                "create_wallet" => {
                    if let (Some(path), Some(name)) = (
                        data.get("path").and_then(|v| v.as_str()),
                        data.get("name").and_then(|v| v.as_str()),
                    ) {
                        let _ = self.create_wallet(PathBuf::from(path), name).await;
                    }
                }
                "close_wallet" => {
                    if let Some(wallet_id) = data.get("wallet_id").and_then(|v| v.as_str()) {
                        let _ = self.close_wallet(wallet_id).await;
                    }
                }
                _ => {}
            },
            _ => {}
        }

        Ok(())
    }

    fn config_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "enabled": {
                    "type": "boolean",
                    "description": "Enable or disable the SQLiteWallet plugin",
                    "default": true
                },
                "default_wallet_path": {
                    "type": "string",
                    "description": "Path to default SQLite wallet file to open on startup"
                }
            }
        }))
    }

    async fn update_config(&mut self, config: serde_json::Value) -> ExtensionResult<()> {
        if let Some(enabled) = config.get("enabled").and_then(|v| v.as_bool()) {
            self.enabled = enabled;
        }

        if let Some(default_path) = config.get("default_wallet_path").and_then(|v| v.as_str()) {
            self.default_wallet_path = Some(PathBuf::from(default_path));
        }

        info!("SQLiteWallet plugin configuration updated");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::tempdir;

    fn create_test_context() -> PluginContext {
        let temp_dir = tempdir().unwrap();
        PluginContext {
            neo_version: "3.6.0".to_string(),
            config_dir: temp_dir.path().to_path_buf(),
            data_dir: temp_dir.path().to_path_buf(),
            shared_data: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    #[tokio::test]
    async fn test_sqlite_wallet_plugin() {
        let mut plugin = SqliteWalletPlugin::new();
        let context = create_test_context();

        // Test initialization
        assert!(plugin.initialize(&context).await.is_ok());

        // Test start
        assert!(plugin.start().await.is_ok());

        // Test wallet creation
        let wallet_path = context.data_dir.join("test_wallet.db3");
        let wallet_id = plugin
            .create_wallet(wallet_path, "Test Wallet")
            .await
            .unwrap();

        assert!(!wallet_id.is_empty());
        assert_eq!(plugin.list_wallets().await.len(), 1);

        // Test wallet closing
        assert!(plugin.close_wallet(&wallet_id).await.is_ok());
        assert_eq!(plugin.list_wallets().await.len(), 0);

        // Test stop
        assert!(plugin.stop().await.is_ok());
    }

    #[tokio::test]
    async fn test_sqlite_wallet_operations() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db3");

        let mut wallet = SqliteWallet::new(db_path);
        assert!(wallet.open().await.is_ok());

        // Test account operations
        let account = SqliteWalletAccount {
            address: "0x1234567890123456789012345678901234567890".to_string(),
            label: "Test Account".to_string(),
            is_default: true,
            lock: false,
            key: Some("encrypted_key".to_string()),
            contract: None,
            extra: None,
        };

        assert!(wallet.add_account(&account).await.is_ok());

        let retrieved = wallet.get_account(&account.address).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().label, "Test Account");

        let all_accounts = wallet.get_all_accounts().await.unwrap();
        assert_eq!(all_accounts.len(), 1);

        assert!(wallet.close().await.is_ok());
    }
}
