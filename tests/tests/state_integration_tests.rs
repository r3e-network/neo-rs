//! State Integration Tests
//!
//! End-to-end tests for the state management layer including:
//! - WorldState operations
//! - StateTrieManager MPT calculations
//! - Mempool operations
//! - State persistence and rollback

use neo_primitives::{UInt160, UInt256};
use neo_tests::mempool::Mempool;
use neo_tests::mempool::MempoolConfig;
use neo_tests::state::{
    MemoryWorldState, StateChanges, StateTrieManager, StorageItem, StorageKey, WorldState,
};

// ============================================================================
// WorldState Tests
// ============================================================================

#[test]
fn test_memory_world_state_creation() {
    let state = MemoryWorldState::new();
    assert_eq!(state.height(), 0);
}

#[test]
fn test_world_state_commit() {
    let mut state = MemoryWorldState::new();

    let mut changes = StateChanges::new();
    let storage_key = StorageKey::new(UInt160::default(), vec![0x01, 0x02]);
    let storage_item = StorageItem::new(vec![0x03, 0x04, 0x05]);
    changes
        .storage
        .insert(storage_key.clone(), Some(storage_item.clone()));

    state.commit(changes).unwrap();
}

#[test]
fn test_world_state_multiple_commits() {
    let mut state = MemoryWorldState::new();

    let mut changes1 = StateChanges::new();
    let key1 = StorageKey::new(UInt160::from([0x01u8; 20]), vec![0x01]);
    changes1
        .storage
        .insert(key1, Some(StorageItem::new(vec![0x11])));
    state.commit(changes1).unwrap();

    let mut changes2 = StateChanges::new();
    let key2 = StorageKey::new(UInt160::from([0x02u8; 20]), vec![0x02]);
    changes2
        .storage
        .insert(key2, Some(StorageItem::new(vec![0x22])));
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

    for block_index in 1u32..=5 {
        let mut changes = StateChanges::new();
        let key = StorageKey::new(UInt160::default(), block_index.to_le_bytes().to_vec());
        let item = StorageItem::new(vec![block_index as u8; 32]);
        changes.storage.insert(key, Some(item));

        let root = trie.apply_changes(block_index, &changes).unwrap();
        roots.push(root);
    }

    for i in 1..roots.len() {
        assert_ne!(
            roots[i],
            roots[i - 1],
            "Block {} should have different root",
            i + 1
        );
    }

    assert_eq!(trie.current_index(), 5);
}

#[test]
fn test_state_trie_reset_to_root() {
    let mut trie = StateTrieManager::new(false);

    let mut changes = StateChanges::new();
    let key = StorageKey::new(UInt160::default(), vec![0x01]);
    let item = StorageItem::new(vec![0x02]);
    changes.storage.insert(key, Some(item));

    let root1 = trie.apply_changes(1, &changes).unwrap();

    let mut changes2 = StateChanges::new();
    let key2 = StorageKey::new(UInt160::default(), vec![0x03]);
    let item2 = StorageItem::new(vec![0x04]);
    changes2.storage.insert(key2, Some(item2));

    let _root2 = trie.apply_changes(2, &changes2).unwrap();
    assert_eq!(trie.current_index(), 2);

    trie.reset_to_root(root1, 1);
    assert_eq!(trie.current_index(), 1);
    assert_eq!(trie.root_hash(), Some(root1));
}

#[test]
fn test_state_trie_empty_changes() {
    let mut trie = StateTrieManager::new(false);

    let changes = StateChanges::new();
    let root = trie.apply_changes(1, &changes).unwrap();

    assert_eq!(trie.current_index(), 1);
    let _ = root;
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

    let mut changes = StateChanges::new();
    let contract = UInt160::from([0x01u8; 20]);
    let key = StorageKey::new(contract, vec![0x01, 0x02]);
    let item = StorageItem::new(vec![0x03, 0x04, 0x05]);
    changes.storage.insert(key.clone(), Some(item.clone()));

    let root = trie.apply_changes(1, &changes).unwrap();
    assert_ne!(root, UInt256::zero());

    world_state.commit(changes).unwrap();
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
    let large_value = vec![0xFFu8; 65536];
    let item = StorageItem::new(large_value.clone());
    assert_eq!(item.value.len(), 65536);
}

#[test]
fn test_state_changes_deletion() {
    let mut state = MemoryWorldState::new();

    let contract = UInt160::from([0x01u8; 20]);

    let mut add_changes = StateChanges::new();
    let storage_key = StorageKey::new(contract, vec![0x01]);
    add_changes
        .storage
        .insert(storage_key.clone(), Some(StorageItem::new(vec![0x02])));
    state.commit(add_changes).unwrap();

    let mut delete_changes = StateChanges::new();
    delete_changes.storage.insert(storage_key, None);
    state.commit(delete_changes).unwrap();
}

#[test]
fn test_multiple_state_changes_batched() {
    let mut state = MemoryWorldState::new();

    let contract = UInt160::from([0x01u8; 20]);
    let mut changes = StateChanges::new();

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
    assert!(
        elapsed.as_secs() < 1,
        "1000 keys took too long: {:?}",
        elapsed
    );
}
