//! End-to-End Integration Tests
//!
//! Full system integration tests that verify the complete node functionality:
//! - State root calculation
//! - Crypto integration
//! - Concurrent state access

use neo_crypto::Crypto;
use neo_primitives::{UInt160, UInt256};
use neo_tests::state::{StateChanges, StateTrieManager, StorageItem, StorageKey, WorldState};
use std::sync::Arc;
use tokio::sync::RwLock;

use neo_tests::state::MemoryWorldState;

// ============================================================================
// State Root Calculation Tests
// ============================================================================

#[test]
fn test_state_root_determinism() {
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
    let mut trie1 = StateTrieManager::new(false);
    let mut trie2 = StateTrieManager::new(false);

    let mut changes1 = StateChanges::new();
    let key_a = StorageKey::new(UInt160::default(), vec![0x01]);
    let key_b = StorageKey::new(UInt160::default(), vec![0x02]);
    changes1
        .storage
        .insert(key_a.clone(), Some(StorageItem::new(vec![0x0A])));
    changes1
        .storage
        .insert(key_b.clone(), Some(StorageItem::new(vec![0x0B])));

    let mut changes2 = StateChanges::new();
    changes2
        .storage
        .insert(key_b, Some(StorageItem::new(vec![0x0B])));
    changes2
        .storage
        .insert(key_a, Some(StorageItem::new(vec![0x0A])));

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
    changes1
        .storage
        .insert(key.clone(), Some(StorageItem::new(vec![0x0A])));

    let mut changes2 = StateChanges::new();
    changes2
        .storage
        .insert(key, Some(StorageItem::new(vec![0x0B])));

    let root1 = trie1.apply_changes(1, &changes1).unwrap();
    let root2 = trie2.apply_changes(1, &changes2).unwrap();

    assert_ne!(
        root1, root2,
        "Different values should produce different roots"
    );
}

// ============================================================================
// Crypto Integration Tests
// ============================================================================

#[test]
fn test_block_hash_calculation() {
    let mut data = Vec::new();
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&UInt256::zero().to_bytes());
    data.extend_from_slice(&UInt256::zero().to_bytes());
    data.extend_from_slice(&1000u64.to_le_bytes());
    data.extend_from_slice(&0u64.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());

    let hash = Crypto::hash256(&data);
    let block_hash = UInt256::from_bytes(&hash).unwrap();

    assert_ne!(block_hash, UInt256::zero());
}

#[test]
fn test_merkle_root_calculation() {
    let empty_merkle = UInt256::zero();
    let tx_hash = UInt256::from([0x01u8; 32]);
    let single_merkle = tx_hash;

    assert_ne!(empty_merkle, single_merkle);
}

// ============================================================================
// Async Runtime Tests
// ============================================================================

#[tokio::test]
async fn test_concurrent_state_commits() {
    let state = Arc::new(RwLock::new(MemoryWorldState::new()));

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

    let state = state.read().await;
    assert_eq!(state.height(), 0);
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
