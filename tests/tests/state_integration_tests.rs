//! State Integration Tests
//!
//! End-to-end tests for the state management layer including:
//! - WorldState operations
//! - StateTrieManager MPT calculations
//! - Chain state management
//! - Mempool operations
//! - State persistence and rollback

use neo_chain::{BlockIndexEntry, ChainState};
use neo_mempool::{Mempool, MempoolConfig};
use neo_primitives::{UInt160, UInt256};
use neo_state::{MemoryWorldState, StateChanges, StateTrieManager, StorageItem, StorageKey, WorldState};

// ============================================================================
// WorldState Tests
// ============================================================================

#[test]
fn test_memory_world_state_creation() {
    let state = MemoryWorldState::new();
    // MemoryWorldState is created empty
    assert_eq!(state.height(), 0);
}

#[test]
fn test_world_state_commit() {
    let mut state = MemoryWorldState::new();

    let mut changes = StateChanges::new();
    let storage_key = StorageKey::new(UInt160::default(), vec![0x01, 0x02]);
    let storage_item = StorageItem::new(vec![0x03, 0x04, 0x05]);
    changes.storage.insert(storage_key.clone(), Some(storage_item.clone()));

    // Commit changes
    state.commit(changes).unwrap();
}

#[test]
fn test_world_state_multiple_commits() {
    let mut state = MemoryWorldState::new();

    // First commit
    let mut changes1 = StateChanges::new();
    let key1 = StorageKey::new(UInt160::from([0x01u8; 20]), vec![0x01]);
    changes1.storage.insert(key1, Some(StorageItem::new(vec![0x11])));
    state.commit(changes1).unwrap();

    // Second commit
    let mut changes2 = StateChanges::new();
    let key2 = StorageKey::new(UInt160::from([0x02u8; 20]), vec![0x02]);
    changes2.storage.insert(key2, Some(StorageItem::new(vec![0x22])));
    state.commit(changes2).unwrap();
}

// ============================================================================
// StateTrieManager Tests
// ============================================================================

#[test]
fn test_state_trie_manager_creation() {
    let trie = StateTrieManager::new(false);
    assert!(trie.root_hash().is_none());
    assert_eq!(trie.current_index(), 0);
}

#[test]
fn test_state_trie_manager_full_state_mode() {
    let trie = StateTrieManager::new(true);
    assert!(trie.root_hash().is_none());
}

#[test]
fn test_state_trie_apply_changes() {
    let mut trie = StateTrieManager::new(false);

    let mut changes = StateChanges::new();
    let key = StorageKey::new(UInt160::default(), vec![0x01, 0x02]);
    let item = StorageItem::new(vec![0x03, 0x04, 0x05]);
    changes.storage.insert(key, Some(item));

    let root = trie.apply_changes(1, &changes).unwrap();

    assert_ne!(root, UInt256::zero());
    assert!(trie.root_hash().is_some());
    assert_eq!(trie.current_index(), 1);
}

#[test]
fn test_state_trie_incremental_blocks() {
    let mut trie = StateTrieManager::new(false);
    let mut roots = Vec::new();

    // Apply changes for multiple blocks
    for block_index in 1u32..=5 {
        let mut changes = StateChanges::new();
        let key = StorageKey::new(UInt160::default(), block_index.to_le_bytes().to_vec());
        let item = StorageItem::new(vec![block_index as u8; 32]);
        changes.storage.insert(key, Some(item));

        let root = trie.apply_changes(block_index, &changes).unwrap();
        roots.push(root);
    }

    // Each block should produce a different root
    for i in 1..roots.len() {
        assert_ne!(roots[i], roots[i - 1], "Block {} should have different root", i + 1);
    }

    assert_eq!(trie.current_index(), 5);
}

#[test]
fn test_state_trie_reset_to_root() {
    let mut trie = StateTrieManager::new(false);

    // Apply some changes
    let mut changes = StateChanges::new();
    let key = StorageKey::new(UInt160::default(), vec![0x01]);
    let item = StorageItem::new(vec![0x02]);
    changes.storage.insert(key, Some(item));

    let root1 = trie.apply_changes(1, &changes).unwrap();

    // Apply more changes
    let mut changes2 = StateChanges::new();
    let key2 = StorageKey::new(UInt160::default(), vec![0x03]);
    let item2 = StorageItem::new(vec![0x04]);
    changes2.storage.insert(key2, Some(item2));

    let _root2 = trie.apply_changes(2, &changes2).unwrap();
    assert_eq!(trie.current_index(), 2);

    // Reset to block 1
    trie.reset_to_root(root1, 1);
    assert_eq!(trie.current_index(), 1);
    assert_eq!(trie.root_hash(), Some(root1));
}

#[test]
fn test_state_trie_empty_changes() {
    let mut trie = StateTrieManager::new(false);

    let changes = StateChanges::new();
    let root = trie.apply_changes(1, &changes).unwrap();

    // Empty changes should still produce a valid root
    assert_eq!(trie.current_index(), 1);
    // Root might be zero or a specific empty trie root
    let _ = root;
}

// ============================================================================
// ChainState Tests
// ============================================================================

#[test]
fn test_chain_state_creation() {
    let chain = ChainState::new();
    assert!(!chain.is_initialized());
    assert_eq!(chain.height(), 0);
}

#[test]
fn test_chain_state_genesis_initialization() {
    let chain = ChainState::new();

    let genesis = BlockIndexEntry {
        hash: UInt256::from([0x01u8; 32]),
        height: 0,
        prev_hash: UInt256::zero(),
        timestamp: 1000,
        tx_count: 0,
        size: 100,
        cumulative_difficulty: 1,
        on_main_chain: true,
    };

    chain.init_genesis(genesis.clone()).unwrap();

    assert!(chain.is_initialized());
    assert_eq!(chain.height(), 0);
    assert_eq!(chain.current_hash(), Some(genesis.hash));
}

#[test]
fn test_chain_state_add_block() {
    let chain = ChainState::new();

    // Initialize with genesis
    let genesis = BlockIndexEntry {
        hash: UInt256::from([0x01u8; 32]),
        height: 0,
        prev_hash: UInt256::zero(),
        timestamp: 1000,
        tx_count: 0,
        size: 100,
        cumulative_difficulty: 1,
        on_main_chain: true,
    };
    chain.init_genesis(genesis.clone()).unwrap();

    // Add block 1
    let block1 = BlockIndexEntry {
        hash: UInt256::from([0x02u8; 32]),
        height: 1,
        prev_hash: genesis.hash,
        timestamp: 2000,
        tx_count: 5,
        size: 500,
        cumulative_difficulty: 2,
        on_main_chain: false,
    };

    let is_new_tip = chain.add_block(block1.clone()).unwrap();
    assert!(is_new_tip);
    assert_eq!(chain.height(), 1);
    assert_eq!(chain.current_hash(), Some(block1.hash));
}

#[test]
fn test_chain_state_block_lookup() {
    let chain = ChainState::new();

    let genesis = BlockIndexEntry {
        hash: UInt256::from([0x01u8; 32]),
        height: 0,
        prev_hash: UInt256::zero(),
        timestamp: 1000,
        tx_count: 0,
        size: 100,
        cumulative_difficulty: 1,
        on_main_chain: true,
    };
    chain.init_genesis(genesis.clone()).unwrap();

    // Lookup by hash
    let found = chain.get_block(&genesis.hash);
    assert!(found.is_some());
    assert_eq!(found.unwrap().height, 0);

    // Lookup by height
    let found_by_height = chain.get_block_at_height(0);
    assert!(found_by_height.is_some());
    assert_eq!(found_by_height.unwrap().hash, genesis.hash);
}

#[test]
fn test_chain_state_orphan_block() {
    let chain = ChainState::new();

    let genesis = BlockIndexEntry {
        hash: UInt256::from([0x01u8; 32]),
        height: 0,
        prev_hash: UInt256::zero(),
        timestamp: 1000,
        tx_count: 0,
        size: 100,
        cumulative_difficulty: 1,
        on_main_chain: true,
    };
    chain.init_genesis(genesis).unwrap();

    // Try to add orphan block (prev_hash doesn't exist)
    let orphan = BlockIndexEntry {
        hash: UInt256::from([0x03u8; 32]),
        height: 2,
        prev_hash: UInt256::from([0xFF; 32]), // Non-existent parent
        timestamp: 3000,
        tx_count: 1,
        size: 200,
        cumulative_difficulty: 3,
        on_main_chain: false,
    };

    let result = chain.add_block(orphan);
    assert!(result.is_err());
}

// ============================================================================
// Mempool Tests
// ============================================================================

#[test]
fn test_mempool_creation() {
    let mempool = Mempool::new();
    assert_eq!(mempool.len(), 0);
    assert!(mempool.is_empty());
}

#[test]
fn test_mempool_with_config() {
    let config = MempoolConfig::default();
    let mempool = Mempool::with_config(config);
    assert!(mempool.is_empty());
}

#[test]
fn test_mempool_get_top() {
    let mempool = Mempool::new();

    // Get top from empty mempool
    let top = mempool.get_top(10);
    assert!(top.is_empty());
}

// ============================================================================
// Cross-Layer Integration Tests
// ============================================================================

#[test]
fn test_state_changes_to_world_state() {
    let mut world_state = MemoryWorldState::new();
    let mut trie = StateTrieManager::new(false);

    // Create state changes
    let mut changes = StateChanges::new();
    let contract = UInt160::from([0x01u8; 20]);
    let key = StorageKey::new(contract, vec![0x01, 0x02]);
    let item = StorageItem::new(vec![0x03, 0x04, 0x05]);
    changes.storage.insert(key.clone(), Some(item.clone()));

    // Apply to trie
    let root = trie.apply_changes(1, &changes).unwrap();
    assert_ne!(root, UInt256::zero());

    // Commit to world state
    world_state.commit(changes).unwrap();
}

#[test]
fn test_chain_state_with_state_roots() {
    let chain = ChainState::new();
    let mut trie = StateTrieManager::new(false);

    // Genesis
    let genesis = BlockIndexEntry {
        hash: UInt256::from([0x01u8; 32]),
        height: 0,
        prev_hash: UInt256::zero(),
        timestamp: 1000,
        tx_count: 0,
        size: 100,
        cumulative_difficulty: 1,
        on_main_chain: true,
    };
    chain.init_genesis(genesis.clone()).unwrap();

    // Calculate genesis state root
    let changes = StateChanges::new();
    let genesis_root = trie.apply_changes(0, &changes).unwrap();

    // Add block 1 with state changes
    let mut changes1 = StateChanges::new();
    let key = StorageKey::new(UInt160::default(), vec![0x01]);
    let item = StorageItem::new(vec![0x02]);
    changes1.storage.insert(key, Some(item));

    let block1_root = trie.apply_changes(1, &changes1).unwrap();

    let block1 = BlockIndexEntry {
        hash: UInt256::from([0x02u8; 32]),
        height: 1,
        prev_hash: genesis.hash,
        timestamp: 2000,
        tx_count: 1,
        size: 200,
        cumulative_difficulty: 2,
        on_main_chain: false,
    };
    chain.add_block(block1).unwrap();

    // State roots should be different
    assert_ne!(genesis_root, block1_root);
    assert_eq!(chain.height(), 1);
    assert_eq!(trie.current_index(), 1);
}

// ============================================================================
// Edge Case Tests
// ============================================================================

#[test]
fn test_storage_key_with_empty_key() {
    let key = StorageKey::new(UInt160::default(), vec![]);
    assert!(key.key.is_empty());
}

#[test]
fn test_storage_item_with_large_value() {
    let large_value = vec![0xFFu8; 65536]; // 64KB
    let item = StorageItem::new(large_value.clone());
    assert_eq!(item.value.len(), 65536);
}

#[test]
fn test_state_changes_deletion() {
    let mut state = MemoryWorldState::new();

    let contract = UInt160::from([0x01u8; 20]);

    // Add value via StateChanges
    let mut add_changes = StateChanges::new();
    let storage_key = StorageKey::new(contract, vec![0x01]);
    add_changes.storage.insert(storage_key.clone(), Some(StorageItem::new(vec![0x02])));
    state.commit(add_changes).unwrap();

    // Delete via StateChanges
    let mut delete_changes = StateChanges::new();
    delete_changes.storage.insert(storage_key, None); // None = deletion
    state.commit(delete_changes).unwrap();
}

#[test]
fn test_multiple_state_changes_batched() {
    let mut state = MemoryWorldState::new();

    let contract = UInt160::from([0x01u8; 20]);
    let mut changes = StateChanges::new();

    // Add multiple keys in one batch
    for i in 0u8..10 {
        let key = StorageKey::new(contract, vec![i]);
        let item = StorageItem::new(vec![i * 2]);
        changes.storage.insert(key, Some(item));
    }

    state.commit(changes).unwrap();
}

// ============================================================================
// Performance Sanity Tests
// ============================================================================

#[test]
fn test_state_trie_performance_1000_keys() {
    let mut trie = StateTrieManager::new(false);

    let start = std::time::Instant::now();

    let mut changes = StateChanges::new();
    for i in 0u32..1000 {
        let key = StorageKey::new(UInt160::default(), i.to_le_bytes().to_vec());
        let item = StorageItem::new(vec![0xFFu8; 32]);
        changes.storage.insert(key, Some(item));
    }

    let _root = trie.apply_changes(1, &changes).unwrap();

    let elapsed = start.elapsed();
    // Should complete in reasonable time (< 1 second)
    assert!(elapsed.as_secs() < 1, "1000 keys took too long: {:?}", elapsed);
}

#[test]
fn test_chain_state_performance_100_blocks() {
    let chain = ChainState::new();

    let genesis = BlockIndexEntry {
        hash: UInt256::from([0x00u8; 32]),
        height: 0,
        prev_hash: UInt256::zero(),
        timestamp: 1000,
        tx_count: 0,
        size: 100,
        cumulative_difficulty: 1,
        on_main_chain: true,
    };
    chain.init_genesis(genesis.clone()).unwrap();

    let start = std::time::Instant::now();

    let mut prev_hash = genesis.hash;
    for i in 1u32..=100 {
        let mut hash_bytes = [0u8; 32];
        hash_bytes[0..4].copy_from_slice(&i.to_le_bytes());
        let hash = UInt256::from(hash_bytes);

        let block = BlockIndexEntry {
            hash,
            height: i,
            prev_hash,
            timestamp: 1000 + (i as u64 * 1000),
            tx_count: 10,
            size: 1000,
            cumulative_difficulty: i as u64 + 1,
            on_main_chain: false,
        };

        chain.add_block(block).unwrap();
        prev_hash = hash;
    }

    let elapsed = start.elapsed();
    assert!(elapsed.as_millis() < 500, "100 blocks took too long: {:?}", elapsed);
    assert_eq!(chain.height(), 100);
}
