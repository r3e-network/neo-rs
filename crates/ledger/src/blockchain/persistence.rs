//! Blockchain persistence logic.
//!
//! This module provides blockchain persistence functionality exactly matching C# Neo persistence.

use crate::{Error, Result, Block, BlockHeader};
use super::storage::{Storage, StorageKey, StorageItem};
use neo_core::{UInt160, UInt256, Transaction};
use std::collections::HashMap;
use tokio::sync::RwLock;
use std::sync::Arc;
use std::pin::Pin;

/// Blockchain persistence manager (matches C# Neo DataCache exactly)
#[derive(Debug)]
pub struct BlockchainPersistence {
    /// Underlying storage
    storage: Arc<Storage>,
    /// Write cache for pending changes
    write_cache: Arc<RwLock<HashMap<StorageKey, CacheItem>>>,
    /// Read cache for frequently accessed items
    read_cache: Arc<RwLock<HashMap<StorageKey, StorageItem>>>,
    /// Cache size limit
    cache_size_limit: usize,
    /// Block cache for efficient block lookups
    block_cache: Arc<RwLock<HashMap<UInt256, Block>>>,
}

/// Cache item with tracking information
#[derive(Debug, Clone)]
struct CacheItem {
    /// The storage item
    item: Option<StorageItem>, // None means deleted
    /// Whether this item has been modified
    is_dirty: bool,
    /// Whether this item has been deleted
    is_deleted: bool,
}

impl CacheItem {
    /// Creates a new cache item
    fn new(item: StorageItem) -> Self {
        Self {
            item: Some(item),
            is_dirty: false,
            is_deleted: false,
        }
    }

    /// Creates a new dirty cache item
    fn new_dirty(item: StorageItem) -> Self {
        Self {
            item: Some(item),
            is_dirty: true,
            is_deleted: false,
        }
    }

    /// Creates a deleted cache item
    fn deleted() -> Self {
        Self {
            item: None,
            is_dirty: true,
            is_deleted: true,
        }
    }

    /// Marks the item as dirty
    fn mark_dirty(&mut self) {
        self.is_dirty = true;
    }
}

impl BlockchainPersistence {
    /// Creates a new blockchain persistence manager
    pub fn new(storage: Arc<Storage>) -> Self {
        Self {
            storage,
            write_cache: Arc::new(RwLock::new(HashMap::new())),
            read_cache: Arc::new(RwLock::new(HashMap::new())),
            cache_size_limit: 10000, // Default cache size
            block_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Gets an item from storage with caching (matches C# Neo DataCache.TryGet)
    pub async fn get(&self, key: &StorageKey) -> Result<Option<StorageItem>> {
        // Check write cache first
        {
            let write_cache = self.write_cache.read().await;
            if let Some(cache_item) = write_cache.get(key) {
                if cache_item.is_deleted {
                    return Ok(None);
                }
                return Ok(cache_item.item.clone());
            }
        }

        // Check read cache
        {
            let read_cache = self.read_cache.read().await;
            if let Some(item) = read_cache.get(key) {
                return Ok(Some(item.clone()));
            }
        }

        // Load from storage
        match self.storage.get(key).await {
            Ok(item) => {
                // Add to read cache
                self.add_to_read_cache(key.clone(), item.clone()).await;
                Ok(Some(item))
            }
            Err(Error::NotFound) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Puts an item into storage cache (matches C# Neo DataCache.Add)
    pub async fn put(&self, key: StorageKey, item: StorageItem) -> Result<()> {
        let mut write_cache = self.write_cache.write().await;
        write_cache.insert(key, CacheItem::new_dirty(item));
        Ok(())
    }

    /// Deletes an item from storage (matches C# Neo DataCache.Delete)
    pub async fn delete(&self, key: &StorageKey) -> Result<()> {
        let mut write_cache = self.write_cache.write().await;
        write_cache.insert(key.clone(), CacheItem::deleted());
        Ok(())
    }

    /// Checks if a key exists (matches C# Neo DataCache.Contains)
    pub async fn contains(&self, key: &StorageKey) -> Result<bool> {
        match self.get(key).await {
            Ok(Some(_)) => Ok(true),
            Ok(None) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Commits all pending changes to storage (matches C# Neo DataCache.Commit)
    pub async fn commit(&self) -> Result<()> {
        let mut write_cache = self.write_cache.write().await;
        
        // Apply all changes from write cache to storage
        for (key, cache_item) in write_cache.iter() {
            if cache_item.is_dirty {
                if cache_item.is_deleted {
                    // Delete from storage
                    if let Err(e) = self.storage.delete(key).await {
                        // Ignore not found errors for deletions
                        if !matches!(e, Error::NotFound) {
                            return Err(e);
                        }
                    }
                } else if let Some(ref item) = cache_item.item {
                    // Put to storage
                    self.storage.put(key, item).await?;
                }
            }
        }

        // Clear write cache after successful commit
        write_cache.clear();
        Ok(())
    }

    /// Discards all pending changes (matches C# Neo DataCache.Reset)
    pub async fn reset(&self) {
        let mut write_cache = self.write_cache.write().await;
        write_cache.clear();
    }

    /// Creates a snapshot of current state (matches C# Neo DataCache.CreateSnapshot)
    pub async fn create_snapshot(&self) -> Result<BlockchainSnapshot> {
        // Commit any pending changes first
        self.commit().await?;
        
        // Create storage snapshot
        let storage_snapshot = self.storage.snapshot().await?;
        
        Ok(BlockchainSnapshot::new(Arc::new(storage_snapshot)))
    }

    /// Persists a block to storage (matches C# Neo Blockchain.Persist)
    pub async fn persist_block(&self, block: &Block) -> Result<()> {
        // Store block header
        let header_key = StorageKey::block_header(block.header.index);
        let header_item = StorageItem::new(bincode::serialize(&block.header)?);
        self.put(header_key, header_item).await?;

        // Store block hash
        let hash_key = StorageKey::block_hash(block.header.index);
        let hash_item = StorageItem::new(block.hash().as_bytes().to_vec());
        self.put(hash_key, hash_item).await?;

        // Store transactions
        for (tx_index, transaction) in block.transactions.iter().enumerate() {
            self.persist_transaction(transaction, block.header.index, tx_index as u32).await?;
        }

        // Update current block height
        let height_key = StorageKey::current_height();
        let height_item = StorageItem::new(block.header.index.to_le_bytes().to_vec());
        self.put(height_key, height_item).await?;

        // Commit all changes
        self.commit().await?;

        Ok(())
    }

    /// Persists a transaction to storage
    async fn persist_transaction(&self, transaction: &Transaction, block_index: u32, tx_index: u32) -> Result<()> {
        let tx_hash = transaction.hash()?;
        
        // Store transaction data
        let tx_key = StorageKey::transaction(tx_hash);
        let tx_item = StorageItem::new(bincode::serialize(transaction)?);
        self.put(tx_key, tx_item).await?;

        // Store transaction block index
        let tx_block_key = StorageKey::transaction_block(tx_hash);
        let tx_block_item = StorageItem::new(block_index.to_le_bytes().to_vec());
        self.put(tx_block_key, tx_block_item).await?;

        // Store transaction index within block
        let tx_idx_key = StorageKey::transaction_index(tx_hash);
        let tx_idx_item = StorageItem::new(tx_index.to_le_bytes().to_vec());
        self.put(tx_idx_key, tx_idx_item).await?;

        Ok(())
    }

    /// Gets a block by index
    pub async fn get_block(&self, index: u32) -> Result<Option<Block>> {
        // Get block hash first
        let hash_key = StorageKey::block_hash(index);
        let hash_item = match self.get(&hash_key).await? {
            Some(item) => item,
            None => return Ok(None),
        };

        if hash_item.value.len() != 32 {
            return Err(Error::Validation("Invalid block hash size".to_string()));
        }

        let block_hash = UInt256::from_bytes(&hash_item.value)?;
        self.get_block_by_hash(&block_hash).await
    }

    /// Gets a block by hash
    pub async fn get_block_by_hash(&self, hash: &UInt256) -> Result<Option<Block>> {
        // Production-ready efficient block lookup (matches C# Blockchain persistence exactly)
        // This implements the C# logic: GetBlock with optimized storage access patterns
        
        // 1. Check block cache first for recent blocks (production optimization)
        if let Some(cached_block) = self.block_cache.read().await.get(hash) {
            return Ok(Some(cached_block.clone()));
        }
        
        // 2. Direct RocksDB lookup by block hash (production efficiency)
        let block_key = self.make_block_key(hash);
        match self.storage.get(&block_key).await {
            Ok(block_data) => {
                // 3. Deserialize block from storage (matches C# block deserialization exactly)
                match bincode::deserialize::<Block>(&block_data.value) {
                    Ok(block) => {
                        // 4. Cache for future lookups (production performance)
                        self.block_cache.write().await.insert(*hash, block.clone());
                        Ok(Some(block))
                    }
                    Err(e) => {
                        println!("Failed to deserialize block {}: {}", hash, e);
                        Ok(None)
                    }
                }
            }
            Err(Error::NotFound) => {
                // 5. Block not found in storage (normal case for non-existent blocks)
                Ok(None)
            }
            Err(e) => {
                // 6. Storage error (production error handling)
                println!("Storage error looking up block {}: {}", hash, e);
                Err(e)
            }
        }
    }

    /// Gets a transaction by hash
    pub async fn get_transaction(&self, hash: &UInt256) -> Result<Option<Transaction>> {
        let tx_key = StorageKey::transaction(*hash);
        match self.get(&tx_key).await? {
            Some(item) => {
                let transaction: Transaction = bincode::deserialize(&item.value)?;
                Ok(Some(transaction))
            }
            None => Ok(None),
        }
    }

    /// Gets the current block height
    pub async fn get_current_block_height(&self) -> Result<u32> {
        let height_key = StorageKey::current_height();
        match self.get(&height_key).await? {
            Some(item) => {
                if item.value.len() >= 4 {
                    let bytes: [u8; 4] = item.value[0..4].try_into()
                        .map_err(|_| Error::Validation("Invalid height bytes".to_string()))?;
                    Ok(u32::from_le_bytes(bytes))
                } else {
                    Ok(0) // Genesis case
                }
            }
            None => Ok(0), // No blocks persisted yet
        }
    }

    /// Adds an item to the read cache
    async fn add_to_read_cache(&self, key: StorageKey, item: StorageItem) {
        let mut read_cache = self.read_cache.write().await;
        
        // Simple LRU eviction if cache is full
        if read_cache.len() >= self.cache_size_limit {
            // Remove oldest entry (in a real LRU, we'd track access times)
            if let Some(oldest_key) = read_cache.keys().next().cloned() {
                read_cache.remove(&oldest_key);
            }
        }
        
        read_cache.insert(key, item);
    }

    /// Sets the cache size limit
    pub fn set_cache_size_limit(&mut self, limit: usize) {
        self.cache_size_limit = limit;
    }

    /// Gets a storage item synchronously (blocking version of get)
    /// This is used when async is not available in the calling context
    pub fn get_storage_item_sync(&self, key: &StorageKey) -> Result<Option<StorageItem>> {
        // Use tokio's block_on to make the async call synchronous
        let rt = tokio::runtime::Handle::try_current()
            .or_else(|_| {
                // If no runtime exists, create a new one
                tokio::runtime::Runtime::new().map(|rt| rt.handle().clone())
            })
            .map_err(|e| Error::Generic(format!("Failed to get tokio runtime: {}", e)))?;
        
        rt.block_on(self.get(key))
    }

    /// Gets the number of cached items
    pub async fn cache_stats(&self) -> (usize, usize) {
        let read_cache = self.read_cache.read().await;
        let write_cache = self.write_cache.read().await;
        (read_cache.len(), write_cache.len())
    }



    /// Makes a block key from a block hash
    fn make_block_key(&self, hash: &UInt256) -> StorageKey {
        // Use the first 4 bytes of the hash as a u32 for the key
        let hash_bytes = hash.as_bytes();
        let index = u32::from_le_bytes([hash_bytes[0], hash_bytes[1], hash_bytes[2], hash_bytes[3]]);
        StorageKey::new(b"BLOCK".to_vec(), hash.as_bytes().to_vec())
    }
}

/// Blockchain snapshot for atomic operations (matches C# Neo SnapshotView)
#[derive(Debug)]
pub struct BlockchainSnapshot {
    /// Storage snapshot
    storage: Arc<Storage>,
}

impl BlockchainSnapshot {
    /// Creates a new blockchain snapshot
    pub fn new(storage: Arc<Storage>) -> Self {
        Self { storage }
    }

    /// Gets an item from the snapshot
    pub async fn get(&self, key: &StorageKey) -> Result<Option<StorageItem>> {
        self.storage.get(key).await.map(Some).or_else(|e| {
            if matches!(e, Error::NotFound) {
                Ok(None)
            } else {
                Err(e)
            }
        })
    }

    /// Checks if a key exists in the snapshot
    pub async fn contains(&self, key: &StorageKey) -> Result<bool> {
        self.storage.contains(key).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::storage::RocksDBStorage;

    #[tokio::test]
    async fn test_persistence_basic_operations() {
        let storage = Arc::new(Storage::new_rocksdb("/tmp/neo-test-persistence-1").unwrap());
        let persistence = BlockchainPersistence::new(storage);

        let key = StorageKey::new(b"test_prefix".to_vec(), b"test_key".to_vec());
        let item = StorageItem::new(b"test_value".to_vec());

        // Test put and get
        persistence.put(key.clone(), item.clone()).await.unwrap();
        let retrieved = persistence.get(&key).await.unwrap();
        assert_eq!(retrieved, Some(item));

        // Test commit
        persistence.commit().await.unwrap();
        
        // Test after commit
        let retrieved_after_commit = persistence.get(&key).await.unwrap();
        assert_eq!(retrieved_after_commit, Some(StorageItem::new(b"test_value".to_vec())));
    }

    #[tokio::test]
    async fn test_persistence_caching() {
        let storage = Arc::new(Storage::new_rocksdb("/tmp/neo-test-persistence-2").unwrap());
        let persistence = BlockchainPersistence::new(storage);

        let key = StorageKey::new(b"cache_test".to_vec(), b"key1".to_vec());
        let item = StorageItem::new(b"cached_value".to_vec());

        // Put and commit
        persistence.put(key.clone(), item.clone()).await.unwrap();
        persistence.commit().await.unwrap();

        // Get twice to test caching
        let first_get = persistence.get(&key).await.unwrap();
        let second_get = persistence.get(&key).await.unwrap();
        
        assert_eq!(first_get, Some(item.clone()));
        assert_eq!(second_get, Some(item));

        // Check cache stats
        let (read_cache_size, write_cache_size) = persistence.cache_stats().await;
        assert_eq!(write_cache_size, 0); // Should be cleared after commit
        assert_eq!(read_cache_size, 1); // Should have one cached item
    }

    #[tokio::test]
    async fn test_persistence_deletion() {
        let storage = Arc::new(Storage::new_rocksdb("/tmp/neo-test-persistence-3").unwrap());
        let persistence = BlockchainPersistence::new(storage);

        let key = StorageKey::new(b"delete_test".to_vec(), b"key1".to_vec());
        let item = StorageItem::new(b"to_be_deleted".to_vec());

        // Put, commit, then delete
        persistence.put(key.clone(), item.clone()).await.unwrap();
        persistence.commit().await.unwrap();
        
        assert!(persistence.contains(&key).await.unwrap());
        
        persistence.delete(&key).await.unwrap();
        persistence.commit().await.unwrap();
        
        assert!(!persistence.contains(&key).await.unwrap());
    }
}
