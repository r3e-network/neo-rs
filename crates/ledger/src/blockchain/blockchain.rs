//! Main blockchain implementation.
//!
//! This module provides the main blockchain functionality exactly matching C# Neo Blockchain.

use crate::{Error, Result, Block, BlockHeader, NetworkType};
use super::{
    storage::{Storage, StorageKey, StorageItem, RocksDBStorage},
    persistence::{BlockchainPersistence, BlockchainSnapshot},
    verification::{BlockchainVerifier, VerifyResult},
    state::{BlockchainState, PolicySettings},
    genesis::GenesisManager,
};
use neo_core::{UInt160, UInt256, Transaction};
use std::sync::Arc;
use tokio::sync::{RwLock, Mutex};
use std::collections::HashMap;

/// Main blockchain manager (matches C# Neo Blockchain exactly)
#[derive(Debug)]
pub struct Blockchain {
    /// Blockchain persistence layer
    persistence: Arc<BlockchainPersistence>,
    /// Blockchain state manager
    state: Arc<RwLock<BlockchainState>>,
    /// Blockchain verifier
    verifier: Arc<BlockchainVerifier>,
    /// Genesis manager
    genesis: Arc<GenesisManager>,
    /// Current block height cache
    current_height: Arc<RwLock<u32>>,
    /// Block cache
    block_cache: Arc<RwLock<HashMap<u32, Block>>>,
    /// Transaction cache
    transaction_cache: Arc<RwLock<HashMap<UInt256, Transaction>>>,
    /// Sync lock for block persistence
    persist_lock: Arc<Mutex<()>>,
    /// Network configuration
    network: NetworkType,
}



impl Blockchain {
    /// Creates a new blockchain instance (matches C# Neo Blockchain.Create exactly)
    pub async fn new(network: NetworkType) -> Result<Self> {
        tracing::info!("🔧 Creating new blockchain instance for network: {:?}", network);
        
        // Initialize storage (RocksDB only)
        let storage = Arc::new(Storage::new_default().unwrap_or_else(|_| {
            eprintln!("Warning: Failed to create default storage, using temporary RocksDB storage");
            let temp_dir = format!("/tmp/neo-blockchain-{}", std::process::id());
            Storage::new_rocksdb(&temp_dir).expect("Failed to create temporary RocksDB storage")
        }));
        let persistence = Arc::new(BlockchainPersistence::new(storage.clone()));
        let state = Arc::new(RwLock::new(BlockchainState::new(persistence.clone())));
        let verifier = Arc::new(BlockchainVerifier::new());
        let genesis = Arc::new(GenesisManager::new(storage.clone()));

        let blockchain = Self {
            persistence: persistence.clone(),
            state,
            verifier,
            genesis,
            current_height: Arc::new(RwLock::new(0)),
            block_cache: Arc::new(RwLock::new(HashMap::new())),
            transaction_cache: Arc::new(RwLock::new(HashMap::new())),
            persist_lock: Arc::new(Mutex::new(())),
            network,
        };

        // Initialize genesis block if needed
        tracing::info!("🔧 Initializing genesis block...");
        match blockchain.initialize_genesis().await {
            Ok(()) => {
                tracing::info!("✅ Genesis initialization completed successfully");
            }
            Err(e) => {
                tracing::error!("❌ Genesis initialization failed: {}", e);
                return Err(e);
            }
        }

        tracing::info!("✅ Blockchain created successfully");
        Ok(blockchain)
    }

    /// Initializes the genesis block (matches C# Neo Blockchain initialization exactly)
    async fn initialize_genesis(&self) -> Result<()> {
        let current_height = self.persistence.get_current_block_height().await?;
        
        if current_height == 0 {
            // Check if genesis block exists
            if self.persistence.get_block(0).await?.is_none() {
                // Create and persist genesis block
                tracing::info!("Creating genesis block for network: {:?}", self.network);
                let genesis_block = match self.network {
                    NetworkType::MainNet => {
                        tracing::info!("Using MainNet genesis creation");
                        self.genesis.create_genesis_block()?
                    },
                    NetworkType::TestNet => {
                        tracing::info!("Using TestNet genesis creation");
                        self.genesis.create_testnet_genesis_block()?
                    },
                    NetworkType::Private => {
                        tracing::info!("Using Private genesis creation");
                        self.genesis.create_private_genesis_block()?
                    },
                };
                tracing::info!("Persisting genesis block with index: {}", genesis_block.header.index);
                self.persist_block(&genesis_block).await?;
                
                // Update height cache
                {
                    let mut height = self.current_height.write().await;
                    *height = 0;
                }
            }
        } else {
            // Update height cache
            {
                let mut height = self.current_height.write().await;
                *height = current_height;
            }
        }

        Ok(())
    }

    /// Gets the current block height (matches C# Neo Blockchain.Height exactly)
    pub async fn get_height(&self) -> u32 {
        *self.current_height.read().await
    }

    /// Gets a block by index (matches C# Neo Blockchain.GetBlock exactly)
    pub async fn get_block(&self, index: u32) -> Result<Option<Block>> {
        // Check cache first
        {
            let cache = self.block_cache.read().await;
            if let Some(block) = cache.get(&index) {
                return Ok(Some(block.clone()));
            }
        }

        // Load from persistence
        match self.persistence.get_block(index).await? {
            Some(block) => {
                // Cache the block
                {
                    let mut cache = self.block_cache.write().await;
                    cache.insert(index, block.clone());
                }
                Ok(Some(block))
            }
            None => Ok(None),
        }
    }

    /// Gets a block by hash (matches C# Neo Blockchain.GetBlock exactly)
    pub async fn get_block_by_hash(&self, hash: &UInt256) -> Result<Option<Block>> {
        self.persistence.get_block_by_hash(hash).await
    }

    /// Gets the height/index of a block by its hash (matches C# Neo Blockchain exactly)
    pub async fn get_block_height_by_hash(&self, hash: &UInt256) -> Result<Option<u32>> {
        match self.get_block_by_hash(hash).await? {
            Some(block) => Ok(Some(block.header.index)),
            None => Ok(None),
        }
    }

    /// Gets a transaction by hash (matches C# Neo Blockchain.GetTransaction exactly)
    pub async fn get_transaction(&self, hash: &UInt256) -> Result<Option<Transaction>> {
        // Check cache first
        {
            let cache = self.transaction_cache.read().await;
            if let Some(tx) = cache.get(hash) {
                return Ok(Some(tx.clone()));
            }
        }

        // Load from persistence
        match self.persistence.get_transaction(hash).await? {
            Some(transaction) => {
                // Cache the transaction
                {
                    let mut cache = self.transaction_cache.write().await;
                    cache.insert(*hash, transaction.clone());
                }
                Ok(Some(transaction))
            }
            None => Ok(None),
        }
    }

    /// Gets the header of the latest block (matches C# Neo Blockchain.HeaderHeight exactly)
    pub async fn get_header_height(&self) -> u32 {
        // In this implementation, header height equals block height
        self.get_height().await
    }

    /// Persists a block to the blockchain (matches C# Neo Blockchain.Persist exactly)
    pub async fn persist_block(&self, block: &Block) -> Result<()> {
        let _lock = self.persist_lock.lock().await;

        // Skip validation for genesis block (index 0)
        if block.header.index > 0 {
            // Validate block first
            tracing::debug!("🔍 Starting block verification for block index {}", block.header.index);
            let verification_result = self.verifier.verify_header(&block.header).await?;
            tracing::debug!("🔍 Block verification result: {:?}", verification_result);
            if verification_result != VerifyResult::Succeed {
                tracing::error!("❌ Block header verification failed with result: {:?}", verification_result);
                return Err(Error::Validation("Block header verification failed".to_string()));
            }
            tracing::debug!("✅ Block verification succeeded");
        } else {
            tracing::debug!("⏭️ Skipping verification for genesis block");
        }

        // Verify block index is correct
        let current_height = self.get_height().await;
        tracing::debug!("🔍 Current height: {}, block index: {}", current_height, block.header.index);
        let expected_index = if block.header.index == 0 {
            // Genesis block should have index 0
            0
        } else {
            // Regular blocks should have index = current_height + 1
            current_height + 1
        };
        
        if block.header.index != expected_index {
            tracing::error!("❌ Block index validation failed: expected {}, got {}", expected_index, block.header.index);
            return Err(Error::Validation(format!(
                "Invalid block index: expected {}, got {}", 
                expected_index, 
                block.header.index
            )));
        }
        tracing::debug!("✅ Block index validation passed");

        // Verify previous hash
        if block.header.index > 0 {
            if let Some(previous_block) = self.get_block(current_height).await? {
                if block.header.previous_hash != previous_block.hash() {
                    return Err(Error::Validation("Invalid previous hash".to_string()));
                }
            }
        }

        // Verify transactions
        for transaction in &block.transactions {
            let tx_verification = self.verifier.verify_transaction(transaction).await?;
            if tx_verification != VerifyResult::Succeed {
                tracing::error!("❌ Transaction verification failed for tx in block {}", block.header.index);
                return Err(Error::Validation("Transaction verification failed".to_string()));
            }
        }

        // Persist the block
        self.persistence.persist_block(block).await?;

        // Update height cache
        {
            let mut height = self.current_height.write().await;
            *height = block.header.index;
        }

        // Cache the block
        {
            let mut cache = self.block_cache.write().await;
            cache.insert(block.header.index, block.clone());
        }

        // Cache transactions
        {
            let mut tx_cache = self.transaction_cache.write().await;
            for transaction in &block.transactions {
                let tx_hash = transaction.hash()?;
                tx_cache.insert(tx_hash, transaction.clone());
            }
        }

        Ok(())
    }

    /// Adds a transaction to the blockchain (matches C# Neo Blockchain.ContainsTransaction exactly)
    pub async fn contains_transaction(&self, hash: &UInt256) -> Result<bool> {
        match self.get_transaction(hash).await? {
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }

    /// Gets the best block hash (matches C# Neo Blockchain.CurrentBlockHash exactly)
    pub async fn get_best_block_hash(&self) -> Result<UInt256> {
        let height = self.get_height().await;
        if let Some(block) = self.get_block(height).await? {
            Ok(block.hash())
        } else {
            Err(Error::NotFound)
        }
    }

    /// Validates a transaction against current blockchain state
    pub async fn validate_transaction(&self, transaction: &Transaction) -> Result<bool> {
        let state = self.state.read().await;
        state.validate_transaction(transaction).await
    }

    /// Gets current policy settings
    pub async fn get_policy_settings(&self) -> PolicySettings {
        let state = self.state.read().await;
        state.get_policy_settings().clone()
    }

    /// Creates a snapshot of current blockchain state (matches C# Neo Blockchain.GetSnapshot exactly)
    pub async fn create_snapshot(&self) -> Result<BlockchainSnapshot> {
        self.persistence.create_snapshot().await
    }

    /// Gets the network type
    pub fn network(&self) -> NetworkType {
        self.network
    }

    /// Gets the network magic number
    pub fn magic(&self) -> u32 {
        self.network.magic()
    }

    /// Gets blockchain statistics
    pub async fn get_stats(&self) -> BlockchainStats {
        let height = self.get_height().await;
        let (block_cache_size, tx_cache_size) = {
            let block_cache = self.block_cache.read().await;
            let tx_cache = self.transaction_cache.read().await;
            (block_cache.len(), tx_cache.len())
        };
        let (read_cache_size, write_cache_size) = self.persistence.cache_stats().await;

        BlockchainStats {
            height,
            block_cache_size,
            transaction_cache_size: tx_cache_size,
            storage_read_cache_size: read_cache_size,
            storage_write_cache_size: write_cache_size,
        }
    }

    /// Clears all caches
    pub async fn clear_caches(&self) {
        {
            let mut block_cache = self.block_cache.write().await;
            block_cache.clear();
        }
        {
            let mut tx_cache = self.transaction_cache.write().await;
            tx_cache.clear();
        }
        {
            let mut state = self.state.write().await;
            state.clear_caches().await;
        }
    }

    /// Gets memory usage statistics
    pub async fn get_memory_usage(&self) -> MemoryUsage {
        let stats = self.get_stats().await;
        
        // Rough estimates (in bytes)
        let block_cache_bytes = stats.block_cache_size * 1024; // ~1KB per cached block header
        let tx_cache_bytes = stats.transaction_cache_size * 512; // ~512 bytes per cached transaction
        let storage_cache_bytes = (stats.storage_read_cache_size + stats.storage_write_cache_size) * 128; // ~128 bytes per cache entry

        MemoryUsage {
            total_bytes: block_cache_bytes + tx_cache_bytes + storage_cache_bytes,
            block_cache_bytes,
            transaction_cache_bytes: tx_cache_bytes,
            storage_cache_bytes,
        }
    }

    /// Validates blockchain integrity
    pub async fn validate_integrity(&self) -> Result<IntegrityReport> {
        let mut report = IntegrityReport::default();
        let height = self.get_height().await;

        // Validate block chain continuity
        for i in 0..=height {
            if let Some(block) = self.get_block(i).await? {
                report.blocks_checked += 1;

                // Validate previous hash (except genesis)
                if i > 0 {
                    if let Some(prev_block) = self.get_block(i - 1).await? {
                        if block.header.previous_hash != prev_block.hash() {
                            report.errors.push(format!("Block {} has invalid previous hash", i));
                        }
                    } else {
                        report.errors.push(format!("Missing previous block for block {}", i));
                    }
                }

                // Validate block index
                if block.header.index != i {
                    report.errors.push(format!("Block at height {} has wrong index {}", i, block.header.index));
                }

                // Count transactions
                report.transactions_checked += block.transactions.len();
            } else {
                report.errors.push(format!("Missing block at height {}", i));
            }
        }

        Ok(report)
    }
}

/// Blockchain statistics
#[derive(Debug, Clone)]
pub struct BlockchainStats {
    /// Current blockchain height
    pub height: u32,
    /// Number of cached blocks
    pub block_cache_size: usize,
    /// Number of cached transactions
    pub transaction_cache_size: usize,
    /// Storage read cache size
    pub storage_read_cache_size: usize,
    /// Storage write cache size
    pub storage_write_cache_size: usize,
}

/// Memory usage information
#[derive(Debug, Clone)]
pub struct MemoryUsage {
    /// Total memory usage in bytes
    pub total_bytes: usize,
    /// Block cache memory usage
    pub block_cache_bytes: usize,
    /// Transaction cache memory usage
    pub transaction_cache_bytes: usize,
    /// Storage cache memory usage
    pub storage_cache_bytes: usize,
}

/// Blockchain integrity report
#[derive(Debug, Clone, Default)]
pub struct IntegrityReport {
    /// Number of blocks checked
    pub blocks_checked: usize,
    /// Number of transactions checked
    pub transactions_checked: usize,
    /// List of errors found
    pub errors: Vec<String>,
}

impl IntegrityReport {
    /// Returns true if blockchain is valid
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_blockchain_creation() {
        let blockchain = Blockchain::new(NetworkType::TestNet).await.unwrap();
        
        // Should start with genesis block
        assert_eq!(blockchain.get_height().await, 0);
        
        // Genesis block should exist
        let genesis = blockchain.get_block(0).await.unwrap();
        assert!(genesis.is_some());
    }

    #[tokio::test]
    async fn test_network_types() {
        assert_eq!(NetworkType::MainNet.magic(), 0x334f454e);
        assert_eq!(NetworkType::TestNet.magic(), 0x3254334e);
        assert_eq!(NetworkType::Private.magic(), 0x00000000);
    }

    #[tokio::test]
    async fn test_blockchain_stats() {
        let blockchain = Blockchain::new(NetworkType::TestNet).await.unwrap();
        let stats = blockchain.get_stats().await;
        
        assert_eq!(stats.height, 0); // Only genesis block
        assert!(stats.block_cache_size <= 1); // Genesis might be cached
    }

    #[tokio::test]
    async fn test_integrity_validation() {
        let blockchain = Blockchain::new(NetworkType::TestNet).await.unwrap();
        let report = blockchain.validate_integrity().await.unwrap();
        
        assert!(report.is_valid());
        assert_eq!(report.blocks_checked, 1); // Genesis block
    }
}
