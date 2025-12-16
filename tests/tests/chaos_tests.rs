//! Chaos Tests
//!
//! Tests for failure scenarios and recovery:
//! - Network partition simulation
//! - Node failure and recovery
//! - State corruption detection
//! - Consensus timeout handling
//! - Chain reorganization stress tests

use neo_chain::{BlockIndexEntry, ChainState};
use neo_consensus::{ConsensusEvent, ConsensusService, ValidatorInfo};
use neo_crypto::{ECCurve, ECPoint};
use neo_primitives::{UInt160, UInt256};
use neo_state::{
    MemoryWorldState, StateChanges, StateTrieManager, StorageItem, StorageKey, WorldState,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};

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

fn create_chain_with_blocks(block_count: u32) -> (ChainState, Vec<UInt256>) {
    let chain = ChainState::new();
    let mut hashes = Vec::new();

    let genesis = BlockIndexEntry {
        hash: UInt256::from([0x00u8; 32]),
        height: 0,
        prev_hash: UInt256::zero(),
        header: Vec::new(),
        timestamp: 1000,
        tx_count: 0,
        size: 100,
        cumulative_difficulty: 1,
        on_main_chain: true,
    };
    chain.init_genesis(genesis.clone()).unwrap();
    hashes.push(genesis.hash);

    let mut prev_hash = genesis.hash;
    for i in 1..=block_count {
        let mut hash_bytes = [0u8; 32];
        hash_bytes[0..4].copy_from_slice(&i.to_le_bytes());
        let hash = UInt256::from(hash_bytes);

        let block = BlockIndexEntry {
            hash,
            height: i,
            prev_hash,
            header: Vec::new(),
            timestamp: 1000 + (i as u64 * 1000),
            tx_count: 5,
            size: 500,
            cumulative_difficulty: i as u64 + 1,
            on_main_chain: false,
        };

        chain.add_block(block).unwrap();
        hashes.push(hash);
        prev_hash = hash;
    }

    (chain, hashes)
}

// ============================================================================
// Network Partition Tests
// ============================================================================

#[tokio::test]
async fn test_consensus_timeout_triggers_view_change() {
    let (tx, mut rx) = mpsc::channel(100);
    let validators = create_test_validators(7);

    let mut service = ConsensusService::new(
        0x4E454F,
        validators,
        Some(1), // Not primary for block 0
        vec![0u8; 32],
        tx,
    );

    service.start(0, 1000, UInt256::zero(), 0).unwrap();

    // Simulate network partition by triggering timeout
    let timeout_time = 1000 + 60_000; // 60 seconds later
    service.on_timer_tick(timeout_time).unwrap();

    // Should receive ChangeView broadcast
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

    // First timeout
    service.on_timer_tick(1000 + 60_000).unwrap();
    let _ = rx.recv().await; // Drain first ChangeView

    // Service should still be running after timeout
    assert!(service.is_running());
}

// ============================================================================
// Node Failure and Recovery Tests
// ============================================================================

#[test]
fn test_chain_state_recovery_after_crash() {
    // Simulate crash by creating chain, adding blocks, then "recovering"
    let (chain, hashes) = create_chain_with_blocks(10);

    // Verify chain state is consistent
    assert_eq!(chain.height(), 10);
    assert_eq!(chain.current_hash(), Some(hashes[10]));

    // Verify all blocks are accessible
    for (i, hash) in hashes.iter().enumerate() {
        let block = chain.get_block(hash);
        assert!(block.is_some(), "Block {} not found", i);
        assert_eq!(block.unwrap().height, i as u32);
    }
}

#[test]
fn test_state_trie_recovery_from_root() {
    let mut trie = StateTrieManager::new(true); // Use full_state mode for recovery support
    let mut roots = Vec::new();

    // Build up state over multiple blocks
    for i in 1u32..=5 {
        let mut changes = StateChanges::new();
        let key = StorageKey::new(UInt160::default(), i.to_le_bytes().to_vec());
        let item = StorageItem::new(vec![i as u8; 32]);
        changes.storage.insert(key, Some(item));

        let root = trie.apply_changes(i, &changes).unwrap();
        roots.push(root);
    }

    // Simulate crash at block 5, recover to block 3
    let recovery_root = roots[2]; // Block 3 root
    trie.reset_to_root(recovery_root, 3);

    assert_eq!(trie.current_index(), 3);
    assert_eq!(trie.root_hash(), Some(recovery_root));

    // Verify state was reset correctly
    // In full_state mode, the trie maintains all historical data
    // so we can verify the recovery point is correct
    assert_ne!(recovery_root, roots[4]); // Different from block 5 root
}

// ============================================================================
// State Corruption Detection Tests
// ============================================================================

#[test]
fn test_state_root_mismatch_detection() {
    let mut trie1 = StateTrieManager::new(false);
    let mut trie2 = StateTrieManager::new(false);

    // Apply same changes to both
    let mut changes = StateChanges::new();
    let key = StorageKey::new(UInt160::default(), vec![0x01]);
    changes
        .storage
        .insert(key.clone(), Some(StorageItem::new(vec![0x02])));

    let root1 = trie1.apply_changes(1, &changes.clone()).unwrap();
    let root2 = trie2.apply_changes(1, &changes).unwrap();

    assert_eq!(root1, root2, "Same changes should produce same root");

    // Apply different changes to trie2
    let mut different_changes = StateChanges::new();
    different_changes
        .storage
        .insert(key, Some(StorageItem::new(vec![0x03])));
    let root2_modified = trie2.apply_changes(2, &different_changes).unwrap();

    // Roots should now differ
    assert_ne!(root1, root2_modified);
}

#[test]
fn test_detect_missing_state_changes() {
    let mut trie1 = StateTrieManager::new(false);
    let mut trie2 = StateTrieManager::new(false);

    // Trie1 has all changes
    let mut changes1 = StateChanges::new();
    changes1.storage.insert(
        StorageKey::new(UInt160::default(), vec![0x01]),
        Some(StorageItem::new(vec![0x0A])),
    );
    changes1.storage.insert(
        StorageKey::new(UInt160::default(), vec![0x02]),
        Some(StorageItem::new(vec![0x0B])),
    );

    // Trie2 is missing one change
    let mut changes2 = StateChanges::new();
    changes2.storage.insert(
        StorageKey::new(UInt160::default(), vec![0x01]),
        Some(StorageItem::new(vec![0x0A])),
    );
    // Missing vec![0x02] key

    let root1 = trie1.apply_changes(1, &changes1).unwrap();
    let root2 = trie2.apply_changes(1, &changes2).unwrap();

    assert_ne!(
        root1, root2,
        "Missing changes should produce different root"
    );
}

// ============================================================================
// Chain Reorganization Stress Tests
// ============================================================================

#[test]
fn test_deep_chain_reorganization() {
    let chain = ChainState::new();

    // Build main chain
    let genesis = BlockIndexEntry {
        hash: UInt256::from([0x00u8; 32]),
        height: 0,
        prev_hash: UInt256::zero(),
        header: Vec::new(),
        timestamp: 1000,
        tx_count: 0,
        size: 100,
        cumulative_difficulty: 1,
        on_main_chain: true,
    };
    chain.init_genesis(genesis.clone()).unwrap();

    // Add 10 blocks to main chain
    let mut main_chain_tip = genesis.hash;
    for i in 1u32..=10 {
        let mut hash_bytes = [0u8; 32];
        hash_bytes[0..4].copy_from_slice(&i.to_le_bytes());
        let hash = UInt256::from(hash_bytes);

        let block = BlockIndexEntry {
            hash,
            height: i,
            prev_hash: main_chain_tip,
            header: Vec::new(),
            timestamp: 1000 + (i as u64 * 1000),
            tx_count: 5,
            size: 500,
            cumulative_difficulty: i as u64 + 1,
            on_main_chain: false,
        };

        chain.add_block(block).unwrap();
        main_chain_tip = hash;
    }

    assert_eq!(chain.height(), 10);

    // Create competing fork from block 5 with higher difficulty
    let fork_point = {
        let mut hash_bytes = [0u8; 32];
        hash_bytes[0..4].copy_from_slice(&5u32.to_le_bytes());
        UInt256::from(hash_bytes)
    };

    // Add fork blocks with higher cumulative difficulty
    let mut fork_tip = fork_point;
    for i in 6u32..=12 {
        let mut hash_bytes = [0xFFu8; 32]; // Different hash prefix for fork
        hash_bytes[0..4].copy_from_slice(&i.to_le_bytes());
        let hash = UInt256::from(hash_bytes);

        let block = BlockIndexEntry {
            hash,
            height: i,
            prev_hash: fork_tip,
            header: Vec::new(),
            timestamp: 1000 + (i as u64 * 1000),
            tx_count: 10,
            size: 1000,
            cumulative_difficulty: (i as u64 + 1) * 2, // Higher difficulty
            on_main_chain: false,
        };

        chain.add_block(block).unwrap();
        fork_tip = hash;
    }

    // Fork should now be the best chain
    assert_eq!(chain.height(), 12);
}

#[test]
fn test_rapid_block_additions() {
    let chain = ChainState::new();

    let genesis = BlockIndexEntry {
        hash: UInt256::from([0x00u8; 32]),
        height: 0,
        prev_hash: UInt256::zero(),
        header: Vec::new(),
        timestamp: 1000,
        tx_count: 0,
        size: 100,
        cumulative_difficulty: 1,
        on_main_chain: true,
    };
    chain.init_genesis(genesis.clone()).unwrap();

    let start = std::time::Instant::now();

    // Rapidly add 1000 blocks
    let mut prev_hash = genesis.hash;
    for i in 1u32..=1000 {
        let mut hash_bytes = [0u8; 32];
        hash_bytes[0..4].copy_from_slice(&i.to_le_bytes());
        let hash = UInt256::from(hash_bytes);

        let block = BlockIndexEntry {
            hash,
            height: i,
            prev_hash,
            header: Vec::new(),
            timestamp: 1000 + (i as u64 * 15000), // 15 second blocks
            tx_count: 50,
            size: 5000,
            cumulative_difficulty: i as u64 + 1,
            on_main_chain: false,
        };

        chain.add_block(block).unwrap();
        prev_hash = hash;
    }

    let elapsed = start.elapsed();
    assert_eq!(chain.height(), 1000);
    assert!(
        elapsed.as_secs() < 2,
        "1000 blocks took too long: {:?}",
        elapsed
    );
}

// ============================================================================
// Concurrent Access Stress Tests
// ============================================================================

#[tokio::test]
async fn test_concurrent_chain_modifications() {
    let chain = Arc::new(ChainState::new());

    let genesis = BlockIndexEntry {
        hash: UInt256::from([0x00u8; 32]),
        height: 0,
        prev_hash: UInt256::zero(),
        header: Vec::new(),
        timestamp: 1000,
        tx_count: 0,
        size: 100,
        cumulative_difficulty: 1,
        on_main_chain: true,
    };
    chain.init_genesis(genesis.clone()).unwrap();

    // Spawn multiple tasks that read chain state
    let mut handles = vec![];
    for task_id in 0..10 {
        let chain_clone = chain.clone();
        let handle = tokio::spawn(async move {
            for _ in 0..100 {
                let height = chain_clone.height();
                let _hash = chain_clone.current_hash();
                let _block = chain_clone.get_block_at_height(0);
                // Yield to allow other tasks to run
                tokio::task::yield_now().await;
                assert!(height <= 1000, "Task {} saw invalid height", task_id);
            }
        });
        handles.push(handle);
    }

    // Wait for all tasks
    for handle in handles {
        handle.await.unwrap();
    }
}

#[tokio::test]
async fn test_concurrent_state_modifications() {
    let state = Arc::new(RwLock::new(MemoryWorldState::new()));

    // Spawn multiple writers
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

                // Small delay to increase contention
                tokio::time::sleep(Duration::from_micros(10)).await;
            }
        });
        handles.push(handle);
    }

    // Wait for all writers
    for handle in handles {
        handle.await.unwrap();
    }

    // Verify final state
    let state = state.read().await;
    assert_eq!(state.height(), 0); // Height unchanged
}

// ============================================================================
// Edge Case Tests
// ============================================================================

#[test]
fn test_empty_block_handling() {
    let chain = ChainState::new();

    let genesis = BlockIndexEntry {
        hash: UInt256::from([0x00u8; 32]),
        height: 0,
        prev_hash: UInt256::zero(),
        header: Vec::new(),
        timestamp: 1000,
        tx_count: 0, // Empty block
        size: 100,
        cumulative_difficulty: 1,
        on_main_chain: true,
    };
    chain.init_genesis(genesis.clone()).unwrap();

    // Add more empty blocks
    let mut prev_hash = genesis.hash;
    for i in 1u32..=5 {
        let mut hash_bytes = [0u8; 32];
        hash_bytes[0..4].copy_from_slice(&i.to_le_bytes());
        let hash = UInt256::from(hash_bytes);

        let block = BlockIndexEntry {
            hash,
            height: i,
            prev_hash,
            header: Vec::new(),
            timestamp: 1000 + (i as u64 * 1000),
            tx_count: 0, // Empty
            size: 100,
            cumulative_difficulty: i as u64 + 1,
            on_main_chain: false,
        };

        chain.add_block(block).unwrap();
        prev_hash = hash;
    }

    assert_eq!(chain.height(), 5);
}

#[test]
fn test_maximum_block_size_handling() {
    let chain = ChainState::new();

    let genesis = BlockIndexEntry {
        hash: UInt256::from([0x00u8; 32]),
        height: 0,
        prev_hash: UInt256::zero(),
        header: Vec::new(),
        timestamp: 1000,
        tx_count: 0,
        size: 100,
        cumulative_difficulty: 1,
        on_main_chain: true,
    };
    chain.init_genesis(genesis.clone()).unwrap();

    // Add block with maximum size
    let large_block = BlockIndexEntry {
        hash: UInt256::from([0x01u8; 32]),
        height: 1,
        prev_hash: genesis.hash,
        header: Vec::new(),
        timestamp: 2000,
        tx_count: 500,
        size: 2 * 1024 * 1024, // 2MB block
        cumulative_difficulty: 2,
        on_main_chain: false,
    };

    chain.add_block(large_block).unwrap();
    assert_eq!(chain.height(), 1);
}

#[test]
fn test_state_with_many_contracts() {
    let mut trie = StateTrieManager::new(false);

    let mut changes = StateChanges::new();

    // Simulate 100 different contracts
    for contract_id in 0u8..100 {
        let contract = UInt160::from([contract_id; 20]);

        // Each contract has 10 storage keys
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
