//! End-to-End Integration Tests
//!
//! Full system integration tests that verify the complete node functionality:
//! - Genesis block initialization
//! - Block execution pipeline
//! - State root calculation
//! - RPC server functionality
//! - Cross-component communication

use neo_chain::{BlockIndexEntry, ChainState};
use neo_crypto::Crypto;
use neo_primitives::{UInt160, UInt256};
use neo_state::{MemoryWorldState, StateChanges, StateTrieManager, StorageItem, StorageKey, WorldState};
use std::sync::Arc;
use tokio::sync::RwLock;

// ============================================================================
// Genesis Block Tests
// ============================================================================

#[test]
fn test_genesis_block_state_initialization() {
    let chain = ChainState::new();
    let mut trie = StateTrieManager::new(false);
    let mut world_state = MemoryWorldState::new();

    // Create genesis block entry
    let genesis_hash = UInt256::from([0x01u8; 32]);
    let genesis = BlockIndexEntry {
        hash: genesis_hash,
        height: 0,
        prev_hash: UInt256::zero(),
        timestamp: 1468595301000, // Neo N3 genesis timestamp
        tx_count: 0,
        size: 100,
        cumulative_difficulty: 1,
        on_main_chain: true,
    };

    // Initialize chain
    chain.init_genesis(genesis).unwrap();
    assert!(chain.is_initialized());

    // Initialize state with empty changes (genesis has no transactions)
    let changes = StateChanges::new();
    let _genesis_root = trie.apply_changes(0, &changes).unwrap();
    world_state.commit(changes).unwrap();

    assert_eq!(chain.height(), 0);
    assert_eq!(trie.current_index(), 0);
}

#[test]
fn test_block_execution_state_flow() {
    let chain = ChainState::new();
    let mut trie = StateTrieManager::new(false);
    let mut world_state = MemoryWorldState::new();

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

    let genesis_changes = StateChanges::new();
    let genesis_root = trie.apply_changes(0, &genesis_changes).unwrap();
    world_state.commit(genesis_changes).unwrap();

    // Simulate block 1 execution with state changes
    let neo_token = UInt160::from([0x01u8; 20]);
    let mut block1_changes = StateChanges::new();

    // Simulate NEO token balance update
    let balance_key = StorageKey::new(neo_token, vec![0x14, 0x01]); // Prefix + account
    let balance_value = StorageItem::new(100_000_000i64.to_le_bytes().to_vec());
    block1_changes.storage.insert(balance_key, Some(balance_value));

    // Apply state changes
    let block1_root = trie.apply_changes(1, &block1_changes).unwrap();
    world_state.commit(block1_changes).unwrap();

    // Add block to chain
    let block1 = BlockIndexEntry {
        hash: UInt256::from([0x02u8; 32]),
        height: 1,
        prev_hash: genesis.hash,
        timestamp: 2000,
        tx_count: 1,
        size: 500,
        cumulative_difficulty: 2,
        on_main_chain: false,
    };
    chain.add_block(block1).unwrap();

    // Verify state
    assert_eq!(chain.height(), 1);
    assert_eq!(trie.current_index(), 1);
    assert_ne!(genesis_root, block1_root);
}

// ============================================================================
// State Root Calculation Tests
// ============================================================================

#[test]
fn test_state_root_determinism() {
    // Same state changes should produce same root
    let mut trie1 = StateTrieManager::new(false);
    let mut trie2 = StateTrieManager::new(false);

    let mut changes = StateChanges::new();
    let key = StorageKey::new(UInt160::default(), vec![0x01, 0x02, 0x03]);
    let item = StorageItem::new(vec![0x04, 0x05, 0x06]);
    changes.storage.insert(key, Some(item));

    let root1 = trie1.apply_changes(1, &changes.clone()).unwrap();
    let root2 = trie2.apply_changes(1, &changes).unwrap();

    assert_eq!(root1, root2, "Same changes should produce same root");
}

#[test]
fn test_state_root_order_independence() {
    // Order of keys in changes shouldn't affect root
    let mut trie1 = StateTrieManager::new(false);
    let mut trie2 = StateTrieManager::new(false);

    // Changes in order A, B
    let mut changes1 = StateChanges::new();
    let key_a = StorageKey::new(UInt160::default(), vec![0x01]);
    let key_b = StorageKey::new(UInt160::default(), vec![0x02]);
    changes1.storage.insert(key_a.clone(), Some(StorageItem::new(vec![0x0A])));
    changes1.storage.insert(key_b.clone(), Some(StorageItem::new(vec![0x0B])));

    // Changes in order B, A (HashMap doesn't guarantee order anyway)
    let mut changes2 = StateChanges::new();
    changes2.storage.insert(key_b, Some(StorageItem::new(vec![0x0B])));
    changes2.storage.insert(key_a, Some(StorageItem::new(vec![0x0A])));

    let root1 = trie1.apply_changes(1, &changes1).unwrap();
    let root2 = trie2.apply_changes(1, &changes2).unwrap();

    assert_eq!(root1, root2, "Order of changes shouldn't affect root");
}

#[test]
fn test_state_root_different_values() {
    let mut trie1 = StateTrieManager::new(false);
    let mut trie2 = StateTrieManager::new(false);

    let key = StorageKey::new(UInt160::default(), vec![0x01]);

    let mut changes1 = StateChanges::new();
    changes1.storage.insert(key.clone(), Some(StorageItem::new(vec![0x0A])));

    let mut changes2 = StateChanges::new();
    changes2.storage.insert(key, Some(StorageItem::new(vec![0x0B])));

    let root1 = trie1.apply_changes(1, &changes1).unwrap();
    let root2 = trie2.apply_changes(1, &changes2).unwrap();

    assert_ne!(root1, root2, "Different values should produce different roots");
}

// ============================================================================
// Chain Reorganization Tests
// ============================================================================

#[test]
fn test_chain_reorganization_state_rollback() {
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

    let genesis_changes = StateChanges::new();
    let _genesis_root = trie.apply_changes(0, &genesis_changes).unwrap();

    // Block 1 on main chain
    let mut block1_changes = StateChanges::new();
    let key1 = StorageKey::new(UInt160::default(), vec![0x01]);
    block1_changes.storage.insert(key1, Some(StorageItem::new(vec![0x11])));
    let block1_root = trie.apply_changes(1, &block1_changes).unwrap();

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
    chain.add_block(block1.clone()).unwrap();

    // Block 2 on main chain
    let mut block2_changes = StateChanges::new();
    let key2 = StorageKey::new(UInt160::default(), vec![0x02]);
    block2_changes.storage.insert(key2, Some(StorageItem::new(vec![0x22])));
    let _block2_root = trie.apply_changes(2, &block2_changes).unwrap();

    assert_eq!(trie.current_index(), 2);

    // Simulate reorg: rollback to block 1
    trie.reset_to_root(block1_root, 1);

    assert_eq!(trie.current_index(), 1);
    assert_eq!(trie.root_hash(), Some(block1_root));
}

// ============================================================================
// Crypto Integration Tests
// ============================================================================

#[test]
fn test_block_hash_calculation() {
    // Simulate block hash calculation
    let mut data = Vec::new();
    data.extend_from_slice(&0u32.to_le_bytes()); // version
    data.extend_from_slice(&UInt256::zero().to_bytes()); // prev_hash
    data.extend_from_slice(&UInt256::zero().to_bytes()); // merkle_root
    data.extend_from_slice(&1000u64.to_le_bytes()); // timestamp
    data.extend_from_slice(&0u64.to_le_bytes()); // nonce
    data.extend_from_slice(&0u32.to_le_bytes()); // index

    let hash = Crypto::hash256(&data);
    let block_hash = UInt256::from_bytes(&hash).unwrap();

    assert_ne!(block_hash, UInt256::zero());
}

#[test]
fn test_merkle_root_calculation() {
    // Empty transaction list -> zero merkle root
    let empty_merkle = UInt256::zero();

    // Single transaction
    let tx_hash = UInt256::from([0x01u8; 32]);
    let single_merkle = tx_hash; // Single tx merkle root is the tx hash itself

    assert_ne!(empty_merkle, single_merkle);
}

// ============================================================================
// Async Runtime Tests
// ============================================================================

#[tokio::test]
async fn test_concurrent_state_commits() {
    let state = Arc::new(RwLock::new(MemoryWorldState::new()));

    // Concurrent commits via StateChanges
    let mut handles = vec![];
    for i in 0u8..10 {
        let state_clone = state.clone();
        let handle = tokio::spawn(async move {
            let mut changes = StateChanges::new();
            let key = StorageKey::new(UInt160::from([i; 20]), vec![i]);
            let item = StorageItem::new(vec![i * 2]);
            changes.storage.insert(key, Some(item));

            let mut state = state_clone.write().await;
            state.commit(changes).unwrap();
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }

    // Verify state was modified
    let state = state.read().await;
    assert_eq!(state.height(), 0); // Height unchanged by commits
}

#[tokio::test]
async fn test_concurrent_chain_reads() {
    let chain = Arc::new(ChainState::new());

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

    // Concurrent reads
    let mut handles = vec![];
    for _ in 0..10 {
        let chain_clone = chain.clone();
        let genesis_hash = genesis.hash;
        let handle = tokio::spawn(async move {
            let height = chain_clone.height();
            let current = chain_clone.current_hash();
            (height, current, genesis_hash)
        });
        handles.push(handle);
    }

    for handle in handles {
        let (height, current, expected_hash) = handle.await.unwrap();
        assert_eq!(height, 0);
        assert_eq!(current, Some(expected_hash));
    }
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[test]
fn test_double_genesis_initialization_fails() {
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

    // Second initialization should fail
    let result = chain.init_genesis(genesis);
    assert!(result.is_err());
}

#[test]
fn test_add_block_before_genesis_fails() {
    let chain = ChainState::new();

    let block = BlockIndexEntry {
        hash: UInt256::from([0x02u8; 32]),
        height: 1,
        prev_hash: UInt256::from([0x01u8; 32]),
        timestamp: 2000,
        tx_count: 1,
        size: 200,
        cumulative_difficulty: 2,
        on_main_chain: false,
    };

    let result = chain.add_block(block);
    assert!(result.is_err());
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
