//! Chain state management

use crate::{BlockIndex, BlockIndexEntry, ChainError, ChainResult};
use neo_primitives::UInt256;
use parking_lot::RwLock;
use std::sync::Arc;

/// Chain state snapshot for consistent reads
#[derive(Debug, Clone)]
pub struct ChainStateSnapshot {
    /// Current block height
    pub height: u32,

    /// Current block hash
    pub hash: UInt256,

    /// Previous block hash
    pub prev_hash: UInt256,

    /// Current timestamp
    pub timestamp: u64,

    /// Total transactions processed
    pub total_transactions: u64,
}

/// Main chain state manager
pub struct ChainState {
    /// Block index
    index: Arc<BlockIndex>,

    /// Current chain tip
    tip: RwLock<Option<ChainStateSnapshot>>,

    /// Genesis hash
    genesis_hash: RwLock<Option<UInt256>>,

    /// Whether the chain is initialized
    initialized: RwLock<bool>,
}

impl ChainState {
    /// Create a new chain state
    #[must_use]
    pub fn new() -> Self {
        Self {
            index: Arc::new(BlockIndex::new()),
            tip: RwLock::new(None),
            genesis_hash: RwLock::new(None),
            initialized: RwLock::new(false),
        }
    }

    /// Initialize the chain with genesis block
    pub fn init_genesis(&self, genesis: BlockIndexEntry) -> ChainResult<()> {
        if *self.initialized.read() {
            return Err(ChainError::StateError("Chain already initialized".into()));
        }

        if genesis.height != 0 {
            return Err(ChainError::InvalidHeight {
                expected: 0,
                actual: genesis.height,
            });
        }

        let hash = genesis.hash;

        self.index.add(genesis.clone());
        self.index.set_best(hash, 0);

        *self.genesis_hash.write() = Some(hash);
        *self.tip.write() = Some(ChainStateSnapshot {
            height: 0,
            hash,
            prev_hash: UInt256::zero(),
            timestamp: genesis.timestamp,
            total_transactions: genesis.tx_count as u64,
        });
        *self.initialized.write() = true;

        tracing::info!("Chain initialized with genesis block: {:?}", hash);
        Ok(())
    }

    /// Check if chain is initialized
    pub fn is_initialized(&self) -> bool {
        *self.initialized.read()
    }

    /// Get the genesis hash
    pub fn genesis_hash(&self) -> Option<UInt256> {
        *self.genesis_hash.read()
    }

    /// Get current chain tip
    pub fn tip(&self) -> Option<ChainStateSnapshot> {
        self.tip.read().clone()
    }

    /// Get current height
    pub fn height(&self) -> u32 {
        self.tip.read().as_ref().map_or(0, |t| t.height)
    }

    /// Get current hash
    pub fn current_hash(&self) -> Option<UInt256> {
        self.tip.read().as_ref().map(|t| t.hash)
    }

    /// Get block index reference
    pub fn index(&self) -> &BlockIndex {
        &self.index
    }

    /// Add a block to the chain
    pub fn add_block(&self, block: BlockIndexEntry) -> ChainResult<bool> {
        if !*self.initialized.read() {
            return Err(ChainError::NotInitialized);
        }

        // Check if block already exists
        if self.index.contains(&block.hash) {
            return Err(ChainError::BlockExists(block.hash));
        }

        // Check parent exists
        if !self.index.contains(&block.prev_hash) {
            return Err(ChainError::OrphanBlock(block.prev_hash));
        }

        let parent = self.index.get_by_hash(&block.prev_hash).unwrap();

        // Validate height
        if block.height != parent.height + 1 {
            return Err(ChainError::InvalidHeight {
                expected: parent.height + 1,
                actual: block.height,
            });
        }

        let _block_hash = block.hash;
        let is_new_tip = self.should_switch_to(&block)?;

        // Add to index
        self.index.add(block.clone());

        if is_new_tip {
            self.switch_to_block(&block)?;
        }

        Ok(is_new_tip)
    }

    /// Check if we should switch to a new block as the tip
    fn should_switch_to(&self, block: &BlockIndexEntry) -> ChainResult<bool> {
        let current_tip = self.tip.read();

        match &*current_tip {
            Some(tip) => {
                // Fork choice rule: prefer higher height, then higher cumulative difficulty
                // For dBFT consensus, forks are rare and this handles edge cases
                Ok(block.height > tip.height
                    || (block.height == tip.height
                        && block.cumulative_difficulty > self.get_difficulty_at(&tip.hash)?))
            }
            None => Ok(true),
        }
    }

    /// Get cumulative difficulty at a block
    fn get_difficulty_at(&self, hash: &UInt256) -> ChainResult<u64> {
        self.index
            .get_by_hash(hash)
            .map(|e| e.cumulative_difficulty)
            .ok_or(ChainError::BlockNotFound(*hash))
    }

    /// Switch the chain tip to a new block
    fn switch_to_block(&self, block: &BlockIndexEntry) -> ChainResult<()> {
        let current_tip = self.tip.read().clone();

        if let Some(tip) = current_tip {
            if block.prev_hash != tip.hash {
                // Need to reorganize
                drop(self.tip.read());
                self.reorganize(&tip.hash, &block.hash)?;
            }
        }

        // Update tip
        let total_tx = self.tip.read().as_ref().map_or(0, |t| t.total_transactions);

        *self.tip.write() = Some(ChainStateSnapshot {
            height: block.height,
            hash: block.hash,
            prev_hash: block.prev_hash,
            timestamp: block.timestamp,
            total_transactions: total_tx + block.tx_count as u64,
        });

        self.index.set_best(block.hash, block.height);
        self.index.set_main_chain(&block.hash, true);

        tracing::debug!(
            "Chain tip updated to height {} hash {:?}",
            block.height,
            block.hash
        );
        Ok(())
    }

    /// Perform chain reorganization
    fn reorganize(&self, from_hash: &UInt256, to_hash: &UInt256) -> ChainResult<()> {
        tracing::warn!("Chain reorganization: {:?} -> {:?}", from_hash, to_hash);

        // Find common ancestor
        let from_chain = self.index.get_chain(from_hash, 1000);
        let to_chain = self.index.get_chain(to_hash, 1000);

        let from_set: std::collections::HashSet<_> = from_chain.iter().collect();

        let common_ancestor = to_chain
            .iter()
            .find(|h| from_set.contains(h))
            .ok_or_else(|| ChainError::ReorgFailed("No common ancestor found".into()))?;

        // Mark old chain blocks as not on main chain
        for hash in &from_chain {
            if hash == common_ancestor {
                break;
            }
            self.index.set_main_chain(hash, false);
        }

        // Mark new chain blocks as on main chain
        for hash in to_chain.iter().rev() {
            if hash == common_ancestor {
                continue;
            }
            self.index.set_main_chain(hash, true);
        }

        tracing::info!(
            "Reorganization complete: {} blocks disconnected, {} blocks connected",
            from_chain
                .iter()
                .take_while(|h| *h != common_ancestor)
                .count(),
            to_chain
                .iter()
                .take_while(|h| *h != common_ancestor)
                .count()
        );

        Ok(())
    }

    /// Get block by hash
    pub fn get_block(&self, hash: &UInt256) -> Option<BlockIndexEntry> {
        self.index.get_by_hash(hash)
    }

    /// Get block by height (main chain)
    pub fn get_block_at_height(&self, height: u32) -> Option<BlockIndexEntry> {
        self.index.get_by_height(height)
    }

    /// Get recent block hashes
    pub fn get_recent_hashes(&self, count: usize) -> Vec<UInt256> {
        let tip = match self.current_hash() {
            Some(h) => h,
            None => return vec![],
        };

        self.index.get_chain(&tip, count)
    }

    /// Initialize chain from an arbitrary starting point (for sync nodes)
    ///
    /// This allows a node to start syncing from any block height without
    /// requiring the full chain history from genesis.
    pub fn init_from_block(&self, block: BlockIndexEntry) -> ChainResult<()> {
        if *self.initialized.read() {
            return Err(ChainError::StateError("Chain already initialized".into()));
        }

        let hash = block.hash;
        let height = block.height;

        self.index.add(block.clone());
        self.index.set_best(hash, height);

        *self.genesis_hash.write() = Some(hash); // Use as synthetic genesis
        *self.tip.write() = Some(ChainStateSnapshot {
            height,
            hash,
            prev_hash: block.prev_hash,
            timestamp: block.timestamp,
            total_transactions: block.tx_count as u64,
        });
        *self.initialized.write() = true;

        tracing::info!(
            "Chain initialized from block at height {}: {:?}",
            height,
            hash
        );
        Ok(())
    }
}

impl Default for ChainState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_genesis() -> BlockIndexEntry {
        BlockIndexEntry {
            hash: UInt256::from([1u8; 32]),
            height: 0,
            prev_hash: UInt256::zero(),
            header: Vec::new(),
            timestamp: 1616245200000,
            tx_count: 1,
            size: 100,
            cumulative_difficulty: 1,
            on_main_chain: true,
        }
    }

    fn create_block(hash_byte: u8, height: u32, prev: &BlockIndexEntry) -> BlockIndexEntry {
        let mut hash = [0u8; 32];
        hash[0] = hash_byte;

        BlockIndexEntry {
            hash: UInt256::from(hash),
            height,
            prev_hash: prev.hash,
            header: Vec::new(),
            timestamp: prev.timestamp + 15000,
            tx_count: 5,
            size: 500,
            cumulative_difficulty: prev.cumulative_difficulty + 1,
            on_main_chain: false,
        }
    }

    #[test]
    fn test_init_genesis() {
        let state = ChainState::new();
        let genesis = create_genesis();

        state.init_genesis(genesis.clone()).unwrap();

        assert!(state.is_initialized());
        assert_eq!(state.height(), 0);
        assert_eq!(state.genesis_hash(), Some(genesis.hash));
    }

    #[test]
    fn test_add_blocks() {
        let state = ChainState::new();
        let genesis = create_genesis();

        state.init_genesis(genesis.clone()).unwrap();

        let block1 = create_block(2, 1, &genesis);
        let is_tip = state.add_block(block1.clone()).unwrap();
        assert!(is_tip);
        assert_eq!(state.height(), 1);

        let block2 = create_block(3, 2, &block1);
        state.add_block(block2.clone()).unwrap();
        assert_eq!(state.height(), 2);
    }

    #[test]
    fn test_orphan_block() {
        let state = ChainState::new();
        let genesis = create_genesis();

        state.init_genesis(genesis.clone()).unwrap();

        // Create orphan block (parent doesn't exist)
        let orphan = BlockIndexEntry {
            hash: UInt256::from([99u8; 32]),
            height: 5,
            prev_hash: UInt256::from([88u8; 32]), // Non-existent parent
            header: Vec::new(),
            timestamp: 1616245200000 + 75000,
            tx_count: 1,
            size: 100,
            cumulative_difficulty: 5,
            on_main_chain: false,
        };

        let result = state.add_block(orphan);
        assert!(matches!(result, Err(ChainError::OrphanBlock(_))));
    }
}
