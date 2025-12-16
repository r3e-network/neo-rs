//! Block index for fast lookups

use neo_primitives::UInt256;
use parking_lot::RwLock;
use std::collections::HashMap;

/// Index entry for a block
#[derive(Debug, Clone)]
pub struct BlockIndexEntry {
    /// Block hash
    pub hash: UInt256,

    /// Block height
    pub height: u32,

    /// Previous block hash
    pub prev_hash: UInt256,

    /// Serialized header bytes for this block (Neo N3 `Header` payload).
    ///
    /// This is stored as raw bytes to keep `neo-chain` independent from `neo-core` types,
    /// while still allowing peers to serve `headers` responses correctly.
    pub header: Vec<u8>,

    /// Block timestamp (milliseconds since Unix epoch)
    pub timestamp: u64,

    /// Number of transactions in the block
    pub tx_count: usize,

    /// Block size in bytes
    pub size: usize,

    /// Cumulative difficulty (for fork choice)
    pub cumulative_difficulty: u64,

    /// Is this block on the main chain?
    pub on_main_chain: bool,
}

/// Block index for efficient lookups
pub struct BlockIndex {
    /// Hash to entry mapping
    by_hash: RwLock<HashMap<UInt256, BlockIndexEntry>>,

    /// Height to hash mapping (main chain only)
    by_height: RwLock<HashMap<u32, UInt256>>,

    /// Current best block hash
    best_hash: RwLock<Option<UInt256>>,

    /// Current best block height
    best_height: RwLock<u32>,
}

impl BlockIndex {
    /// Create a new empty block index
    pub fn new() -> Self {
        Self {
            by_hash: RwLock::new(HashMap::new()),
            by_height: RwLock::new(HashMap::new()),
            best_hash: RwLock::new(None),
            best_height: RwLock::new(0),
        }
    }

    /// Add a block to the index
    pub fn add(&self, entry: BlockIndexEntry) {
        let hash = entry.hash;
        let height = entry.height;
        let on_main_chain = entry.on_main_chain;

        self.by_hash.write().insert(hash, entry);

        if on_main_chain {
            self.by_height.write().insert(height, hash);
        }
    }

    /// Get block entry by hash
    pub fn get_by_hash(&self, hash: &UInt256) -> Option<BlockIndexEntry> {
        self.by_hash.read().get(hash).cloned()
    }

    /// Get block hash by height (main chain only)
    pub fn get_hash_by_height(&self, height: u32) -> Option<UInt256> {
        self.by_height.read().get(&height).cloned()
    }

    /// Get block entry by height (main chain only)
    pub fn get_by_height(&self, height: u32) -> Option<BlockIndexEntry> {
        let hash = self.get_hash_by_height(height)?;
        self.get_by_hash(&hash)
    }

    /// Check if a block exists
    pub fn contains(&self, hash: &UInt256) -> bool {
        self.by_hash.read().contains_key(hash)
    }

    /// Get the best (tip) block hash
    pub fn best_hash(&self) -> Option<UInt256> {
        *self.best_hash.read()
    }

    /// Get the best (tip) block height
    pub fn best_height(&self) -> u32 {
        *self.best_height.read()
    }

    /// Get the best block entry
    pub fn best_block(&self) -> Option<BlockIndexEntry> {
        let hash = self.best_hash()?;
        self.get_by_hash(&hash)
    }

    /// Update the best block
    pub fn set_best(&self, hash: UInt256, height: u32) {
        *self.best_hash.write() = Some(hash);
        *self.best_height.write() = height;
    }

    /// Mark a block as on/off main chain
    pub fn set_main_chain(&self, hash: &UInt256, on_main_chain: bool) {
        if let Some(entry) = self.by_hash.write().get_mut(hash) {
            let was_on_chain = entry.on_main_chain;
            entry.on_main_chain = on_main_chain;

            if on_main_chain && !was_on_chain {
                self.by_height.write().insert(entry.height, entry.hash);
            } else if !on_main_chain && was_on_chain {
                self.by_height.write().remove(&entry.height);
            }
        }
    }

    /// Get blocks at a specific height (may include forks)
    pub fn get_blocks_at_height(&self, height: u32) -> Vec<BlockIndexEntry> {
        self.by_hash
            .read()
            .values()
            .filter(|e| e.height == height)
            .cloned()
            .collect()
    }

    /// Get chain of block hashes from given hash back to genesis
    pub fn get_chain(&self, from_hash: &UInt256, max_count: usize) -> Vec<UInt256> {
        let mut chain = Vec::new();
        let mut current = *from_hash;

        let by_hash = self.by_hash.read();
        while chain.len() < max_count {
            if let Some(entry) = by_hash.get(&current) {
                chain.push(current);
                if entry.prev_hash == UInt256::zero() {
                    break; // Genesis
                }
                current = entry.prev_hash;
            } else {
                break;
            }
        }

        chain
    }

    /// Get total number of indexed blocks
    pub fn len(&self) -> usize {
        self.by_hash.read().len()
    }

    /// Check if index is empty
    pub fn is_empty(&self) -> bool {
        self.by_hash.read().is_empty()
    }

    /// Clear the index
    pub fn clear(&self) {
        self.by_hash.write().clear();
        self.by_height.write().clear();
        *self.best_hash.write() = None;
        *self.best_height.write() = 0;
    }
}

impl Default for BlockIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_entry(hash_byte: u8, height: u32, prev_byte: u8) -> BlockIndexEntry {
        let mut hash = [0u8; 32];
        hash[0] = hash_byte;

        let mut prev = [0u8; 32];
        prev[0] = prev_byte;

        BlockIndexEntry {
            hash: UInt256::from(hash),
            height,
            prev_hash: UInt256::from(prev),
            header: Vec::new(),
            timestamp: 1000000 + height as u64 * 15000,
            tx_count: 1,
            size: 100,
            cumulative_difficulty: height as u64,
            on_main_chain: true,
        }
    }

    #[test]
    fn test_add_and_get() {
        let index = BlockIndex::new();
        let entry = create_entry(1, 1, 0);

        index.add(entry.clone());

        assert!(index.contains(&entry.hash));
        assert_eq!(index.get_by_hash(&entry.hash).unwrap().height, 1);
    }

    #[test]
    fn test_height_lookup() {
        let index = BlockIndex::new();

        for i in 0..10 {
            let entry = create_entry(i, i as u32, i.saturating_sub(1));
            index.add(entry);
        }

        assert_eq!(index.len(), 10);

        let entry = index.get_by_height(5).unwrap();
        assert_eq!(entry.height, 5);
    }

    #[test]
    fn test_best_block() {
        let index = BlockIndex::new();
        let entry = create_entry(10, 100, 9);

        index.add(entry.clone());
        index.set_best(entry.hash, entry.height);

        assert_eq!(index.best_height(), 100);
        assert_eq!(index.best_hash(), Some(entry.hash));
    }
}
