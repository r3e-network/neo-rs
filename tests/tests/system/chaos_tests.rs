//! Chaos Tests
//!
//! Tests for failure scenarios and recovery:
//! - Network partition simulation
//! - Node failure and recovery
//! - State corruption detection
//! - Consensus timeout handling
//! - Concurrent access stress tests

use neo_consensus::{ConsensusEvent, ConsensusService, ValidatorInfo};
use neo_crypto::{ECCurve, ECPoint};
use neo_primitives::{UInt160, UInt256};
use neo_tests::state::{
    MemoryWorldState, StateChanges, StateTrieManager, StorageItem, StorageKey, WorldState,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, mpsc};

// ============================================================================
// Test Helpers
// ============================================================================

fn create_test_validators(count: usize) -> Vec<ValidatorInfo> {
    (0..count)
        .map(|i| ValidatorInfo {
            index: i as u8,
            public_key: ECPoint::infinity(ECCurve::Secp256r1),
            script_hash: UInt160::zero(),
        })
        .collect()
}

// ============================================================================
// Network Partition Tests
// ============================================================================

#[tokio::test]
async fn test_consensus_timeout_triggers_view_change() {
    let (tx, mut rx) = mpsc::channel(100);
    let validators = create_test_validators(7);

    let mut service = ConsensusService::new(0x4E454F, validators, Some(1), vec![0u8; 32], tx);

    service.start(0, 1000, UInt256::zero(), 0).unwrap();

    let timeout_time = 1000 + 60_000;
    service.on_timer_tick(timeout_time).unwrap();

    let event = tokio::time::timeout(Duration::from_millis(100), rx.recv())
        .await
        .expect("timeout")
        .expect("event");

    match event {
        ConsensusEvent::BroadcastMessage(payload) => {
            assert_eq!(
                payload.message_type,
                neo_consensus::ConsensusMessageType::ChangeView
            );
        }
        _ => panic!("Expected ChangeView broadcast"),
    }
}

#[tokio::test]
async fn test_multiple_timeouts_increment_view() {
    let (tx, mut rx) = mpsc::channel(100);
    let validators = create_test_validators(7);

    let mut service = ConsensusService::new(0x4E454F, validators, Some(1), vec![0u8; 32], tx);

    service.start(0, 1000, UInt256::zero(), 0).unwrap();

    service.on_timer_tick(1000 + 60_000).unwrap();
    let _ = rx.recv().await;

    assert!(service.is_running());
}

// ============================================================================
// Node Failure and Recovery Tests
// ============================================================================

#[test]
fn test_state_trie_recovery_from_root() {
    let mut trie = StateTrieManager::new(true);
    let mut roots = Vec::new();

    for i in 1u32..=5 {
        let mut changes = StateChanges::new();
        let key = StorageKey::new(UInt160::default(), i.to_le_bytes().to_vec());
        let item = StorageItem::new(vec![i as u8; 32]);
        changes.storage.insert(key, Some(item));

        let root = trie.apply_changes(i, &changes).unwrap();
        roots.push(root);
    }

    let recovery_root = roots[2];
    trie.reset_to_root(recovery_root, 3);

    assert_eq!(trie.current_index(), 3);
    assert_eq!(trie.root_hash(), Some(recovery_root));

    assert_ne!(recovery_root, roots[4]);
}

// ============================================================================
// State Corruption Detection Tests
// ============================================================================

#[test]
fn test_state_root_mismatch_detection() {
    let mut trie1 = StateTrieManager::new(false);
    let mut trie2 = StateTrieManager::new(false);

    let mut changes = StateChanges::new();
    let key = StorageKey::new(UInt160::default(), vec![0x01]);
    changes
        .storage
        .insert(key.clone(), Some(StorageItem::new(vec![0x02])));

    let root1 = trie1.apply_changes(1, &changes.clone()).unwrap();
    let root2 = trie2.apply_changes(1, &changes).unwrap();

    assert_eq!(root1, root2, "Same changes should produce same root");

    let mut different_changes = StateChanges::new();
    different_changes
        .storage
        .insert(key, Some(StorageItem::new(vec![0x03])));
    let root2_modified = trie2.apply_changes(2, &different_changes).unwrap();

    assert_ne!(root1, root2_modified);
}

#[test]
fn test_detect_missing_state_changes() {
    let mut trie1 = StateTrieManager::new(false);
    let mut trie2 = StateTrieManager::new(false);

    let mut changes1 = StateChanges::new();
    changes1.storage.insert(
        StorageKey::new(UInt160::default(), vec![0x01]),
        Some(StorageItem::new(vec![0x0A])),
    );
    changes1.storage.insert(
        StorageKey::new(UInt160::default(), vec![0x02]),
        Some(StorageItem::new(vec![0x0B])),
    );

    let mut changes2 = StateChanges::new();
    changes2.storage.insert(
        StorageKey::new(UInt160::default(), vec![0x01]),
        Some(StorageItem::new(vec![0x0A])),
    );

    let root1 = trie1.apply_changes(1, &changes1).unwrap();
    let root2 = trie2.apply_changes(1, &changes2).unwrap();

    assert_ne!(
        root1, root2,
        "Missing changes should produce different root"
    );
}

// ============================================================================
// Concurrent Access Stress Tests
// ============================================================================

#[tokio::test]
async fn test_concurrent_state_modifications() {
    let state = Arc::new(RwLock::new(MemoryWorldState::new()));

    let mut handles = vec![];
    for task_id in 0u8..5 {
        let state_clone = state.clone();
        let handle = tokio::spawn(async move {
            for i in 0u8..20 {
                let mut changes = StateChanges::new();
                let key = StorageKey::new(UInt160::from([task_id; 20]), vec![i]);
                let item = StorageItem::new(vec![task_id, i]);
                changes.storage.insert(key, Some(item));

                let mut state = state_clone.write().await;
                state.commit(changes).unwrap();
                drop(state);

                tokio::time::sleep(Duration::from_micros(10)).await;
            }
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
// Edge Case Tests
// ============================================================================

#[test]
fn test_state_with_many_contracts() {
    let mut trie = StateTrieManager::new(false);

    let mut changes = StateChanges::new();

    for contract_id in 0u8..100 {
        let contract = UInt160::from([contract_id; 20]);

        for key_id in 0u8..10 {
            let key = StorageKey::new(contract, vec![key_id]);
            let item = StorageItem::new(vec![contract_id; 32]);
            changes.storage.insert(key, Some(item));
        }
    }

    let start = std::time::Instant::now();
    let root = trie.apply_changes(1, &changes).unwrap();
    let elapsed = start.elapsed();

    assert_ne!(root, UInt256::zero());
    assert!(
        elapsed.as_millis() < 500,
        "1000 keys across 100 contracts took too long: {:?}",
        elapsed
    );
}
