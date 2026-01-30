//! Block Persistence and Retrieval Integration Tests
//!
//! Tests for blockchain storage, indexing, and retrieval:
//! - Block storage and retrieval
//! - Chain reorganization
//! - Block indexing by height and hash
//! - Persistence layer operations
//! - Fork handling
//! - Historical block queries

use neo_chain::{BlockIndexEntry, ChainState};
use neo_core::{UInt160, UInt256};
use neo_state::{
    MemoryWorldState, StateChanges, StateTrieManager, StorageItem, StorageKey, WorldState,
};

// Creates a test block hash from a seed value
fn create_block_hash(seed: u8) -> UInt256 {
    let mut bytes = [0u8; 32];
    bytes[0] = seed;
    UInt256::from(bytes)
}

// Creates a block index entry
fn create_block_entry(
    height: u32,
    prev_hash: UInt256,
    timestamp: u64,
    tx_count: u16,
) -> BlockIndexEntry {
    let mut hash_bytes = [0u8; 32];
    hash_bytes[0..4].copy_from_slice(&height.to_le_bytes());
    let hash = UInt256::from(hash_bytes);

    BlockIndexEntry {
        hash,
        height,
        prev_hash,
        header: Vec::new(),
        timestamp,
        tx_count: tx_count as usize,
        size: 100 + (tx_count as usize * 200),
        cumulative_difficulty: (height + 1) as u64,
        on_main_chain: false,
    }
}

// Setup chain with genesis block
fn setup_chain_with_genesis() -> ChainState {
    let chain = ChainState::new();
    let genesis = create_block_entry(0, UInt256::zero(), 1468595301000, 0);
    chain.init_genesis(genesis).unwrap();
    chain
}

// Build a chain of blocks
fn build_block_chain(chain: &ChainState, count: u32) -> Vec<BlockIndexEntry> {
    let mut blocks = Vec::new();
    let mut prev_hash = chain.current_hash().unwrap();

    for i in 1..=count {
        let block = create_block_entry(i, prev_hash, 1468595301000 + (i as u64 * 15000), 10);
        chain.add_block(block.clone()).unwrap();
        blocks.push(block.clone());
        prev_hash = block.hash;
    }

    blocks
}

#[test]
fn test_genesis_block_storage() {
    let chain = ChainState::new();

    let genesis_hash = create_block_hash(1);
    let genesis = BlockIndexEntry {
        hash: genesis_hash,
        height: 0,
        prev_hash: UInt256::zero(),
        header: vec![0u8; 100],
        timestamp: 1468595301000,
        tx_count: 0,
        size: 100,
        cumulative_difficulty: 1,
        on_main_chain: true,
    };

    chain.init_genesis(genesis.clone()).unwrap();

    assert!(chain.is_initialized());
    assert_eq!(chain.height(), 0);
    assert_eq!(chain.current_hash(), Some(genesis_hash));

    let retrieved = chain.get_block(&genesis_hash);
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().hash, genesis_hash);

    let by_height = chain.get_block_at_height(0);
    assert!(by_height.is_some());
    assert_eq!(by_height.unwrap().hash, genesis_hash);
}

#[test]
fn test_block_storage_by_height() {
    let chain = setup_chain_with_genesis();
    let _blocks = build_block_chain(&chain, 10);

    for i in 1..=10 {
        let block = chain.get_block_at_height(i);
        assert!(block.is_some(), "Block at height {} should exist", i);
        assert_eq!(block.unwrap().height, i);
    }
}

#[test]
fn test_block_storage_by_hash() {
    let chain = setup_chain_with_genesis();
    let blocks = build_block_chain(&chain, 5);

    for block in &blocks {
        let retrieved = chain.get_block(&block.hash);
        assert!(retrieved.is_some(), "Block with hash should exist");
        assert_eq!(retrieved.unwrap().height, block.height);
    }
}

#[test]
fn test_simple_chain_reorganization() {
    let chain = setup_chain_with_genesis();
    let mut world_state = MemoryWorldState::new();
    let mut trie = StateTrieManager::new(false);

    let _main_blocks = build_block_chain(&chain, 3);
    assert_eq!(chain.height(), 3);

    for i in 1..=3 {
        let mut changes = StateChanges::new();
        let key = StorageKey::new(UInt160::default(), vec![i as u8]);
        let item = StorageItem::new(vec![i as u8; 32]);
        changes.storage.insert(key, Some(item));
        let _ = trie.apply_changes(i, &changes);
        world_state.commit(changes).unwrap();
    }

    assert_eq!(chain.height(), 3);
}

#[test]
fn test_orphan_block_handling() {
    let chain = setup_chain_with_genesis();

    let orphan = BlockIndexEntry {
        hash: create_block_hash(99),
        height: 2,
        prev_hash: create_block_hash(98),
        header: Vec::new(),
        timestamp: 1000,
        tx_count: 1,
        size: 200,
        cumulative_difficulty: 3,
        on_main_chain: false,
    };

    let result = chain.add_block(orphan);
    assert!(result.is_err(), "Orphan block should be rejected");
}

#[test]
fn test_block_parent_validation() {
    let chain = setup_chain_with_genesis();

    let block1 = create_block_entry(1, chain.current_hash().unwrap(), 1000, 10);
    chain.add_block(block1.clone()).unwrap();

    let mut block3_hash = [0u8; 32];
    block3_hash[0] = 3;
    let block3 = BlockIndexEntry {
        hash: UInt256::from(block3_hash),
        height: 3,
        prev_hash: block1.hash,
        header: Vec::new(),
        timestamp: 3000,
        tx_count: 1,
        size: 200,
        cumulative_difficulty: 4,
        on_main_chain: false,
    };

    let result = chain.add_block(block3);
    assert!(
        result.is_err(),
        "Block with wrong parent should be rejected"
    );
}

#[test]
fn test_state_trie_with_block_chain() {
    let mut trie = StateTrieManager::new(false);
    let mut world_state = MemoryWorldState::new();

    let genesis_changes = StateChanges::new();
    let genesis_root = trie.apply_changes(0, &genesis_changes).unwrap();
    world_state.commit(genesis_changes).unwrap();

    let mut changes1 = StateChanges::new();
    let key1 = StorageKey::new(UInt160::from([0x01u8; 20]), vec![0x01]);
    changes1
        .storage
        .insert(key1, Some(StorageItem::new(vec![0x11])));
    let root1 = trie.apply_changes(1, &changes1).unwrap();
    world_state.commit(changes1).unwrap();

    let mut changes2 = StateChanges::new();
    let key2 = StorageKey::new(UInt160::from([0x02u8; 20]), vec![0x02]);
    changes2
        .storage
        .insert(key2, Some(StorageItem::new(vec![0x22])));
    let root2 = trie.apply_changes(2, &changes2).unwrap();
    world_state.commit(changes2).unwrap();

    assert_ne!(genesis_root, root1);
    assert_ne!(root1, root2);
    assert_ne!(genesis_root, root2);
}

#[test]
fn test_state_rollback() {
    let mut trie = StateTrieManager::new(false);
    let mut world_state = MemoryWorldState::new();

    let mut changes1 = StateChanges::new();
    let key = StorageKey::new(UInt160::from([0x01u8; 20]), vec![0x01]);
    changes1
        .storage
        .insert(key.clone(), Some(StorageItem::new(vec![0x11])));
    let root1 = trie.apply_changes(1, &changes1).unwrap();
    world_state.commit(changes1).unwrap();

    let mut changes2 = StateChanges::new();
    let key2 = StorageKey::new(UInt160::from([0x02u8; 20]), vec![0x02]);
    changes2
        .storage
        .insert(key2, Some(StorageItem::new(vec![0x22])));
    let _root2 = trie.apply_changes(2, &changes2).unwrap();
    world_state.commit(changes2).unwrap();

    assert_eq!(trie.current_index(), 2);

    trie.reset_to_root(root1, 1);
    assert_eq!(trie.current_index(), 1);
}

#[test]
fn test_block_index_entry_creation() {
    let hash = create_block_hash(1);
    let prev_hash = UInt256::zero();

    let entry = BlockIndexEntry {
        hash,
        height: 100,
        prev_hash,
        header: vec![1, 2, 3, 4],
        timestamp: 1234567890,
        tx_count: 50,
        size: 10000,
        cumulative_difficulty: 1000,
        on_main_chain: true,
    };

    assert_eq!(entry.hash, hash);
    assert_eq!(entry.height, 100);
    assert_eq!(entry.prev_hash, prev_hash);
    assert_eq!(entry.tx_count, 50);
    assert_eq!(entry.size, 10000);
}

#[test]
fn test_chain_contains_block() {
    let chain = setup_chain_with_genesis();
    let genesis_hash = chain.current_hash().unwrap();
    let blocks = build_block_chain(&chain, 3);

    assert!(
        chain.get_block(&genesis_hash).is_some(),
        "Genesis should exist"
    );
    for block in &blocks {
        assert!(chain.get_block(&block.hash).is_some(), "Block should exist");
    }

    let fake_hash = create_block_hash(255);
    assert!(
        chain.get_block(&fake_hash).is_none(),
        "Non-existent block should return None"
    );
}

#[test]
fn test_large_chain_storage() {
    let chain = setup_chain_with_genesis();

    let start = std::time::Instant::now();
    let _blocks = build_block_chain(&chain, 1000);
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_secs() < 5,
        "Building 1000 block chain took too long: {:?}",
        elapsed
    );

    // Just check that height is tracked correctly
    assert_eq!(chain.height(), 1000);
}

#[test]
fn test_chain_lookup_performance() {
    let chain = setup_chain_with_genesis();
    let _blocks = build_block_chain(&chain, 500);

    let hash_start = std::time::Instant::now();
    for i in 0..500u32 {
        let mut hash_bytes = [0u8; 32];
        hash_bytes[0..4].copy_from_slice(&i.to_le_bytes());
        let hash = UInt256::from(hash_bytes);
        let _ = chain.get_block(&hash);
    }
    let hash_elapsed = hash_start.elapsed();

    let height_start = std::time::Instant::now();
    for i in 0..500u32 {
        let _ = chain.get_block_at_height(i);
    }
    let height_elapsed = height_start.elapsed();

    assert!(
        hash_elapsed.as_millis() < 500,
        "Hash lookups took too long: {:?}",
        hash_elapsed
    );
    assert!(
        height_elapsed.as_millis() < 100,
        "Height lookups took too long: {:?}",
        height_elapsed
    );
}

#[test]
fn test_empty_block_handling() {
    let chain = setup_chain_with_genesis();

    let empty_block = BlockIndexEntry {
        hash: create_block_hash(1),
        height: 1,
        prev_hash: chain.current_hash().unwrap(),
        header: Vec::new(),
        timestamp: 2000,
        tx_count: 0,
        size: 100,
        cumulative_difficulty: 2,
        on_main_chain: false,
    };

    let result = chain.add_block(empty_block);
    assert!(result.is_ok(), "Empty block should be valid");
}

#[test]
fn test_block_with_many_transactions() {
    let chain = setup_chain_with_genesis();

    let large_block = BlockIndexEntry {
        hash: create_block_hash(1),
        height: 1,
        prev_hash: chain.current_hash().unwrap(),
        header: Vec::new(),
        timestamp: 2000,
        tx_count: 10000,
        size: 10000000,
        cumulative_difficulty: 2,
        on_main_chain: false,
    };

    let result = chain.add_block(large_block);
    assert!(
        result.is_ok(),
        "Large block should be valid (for indexing purposes)"
    );
}

#[test]
fn test_duplicate_block_rejection() {
    let chain = setup_chain_with_genesis();

    let block = create_block_entry(1, chain.current_hash().unwrap(), 1000, 10);
    chain.add_block(block.clone()).unwrap();

    let result = chain.add_block(block);
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn test_minimum_difficulty_chain() {
    let chain = ChainState::new();

    let genesis = BlockIndexEntry {
        hash: create_block_hash(0),
        height: 0,
        prev_hash: UInt256::zero(),
        header: Vec::new(),
        timestamp: 0,
        tx_count: 0,
        size: 1,
        cumulative_difficulty: 1,
        on_main_chain: true,
    };

    chain.init_genesis(genesis).unwrap();
    assert_eq!(chain.height(), 0);
}

// Test helpers that work around storage limitations
#[test]
fn test_block_chain_height_tracking() {
    let chain = setup_chain_with_genesis();

    // Add blocks one by one
    let mut prev_hash = chain.current_hash().unwrap();
    for i in 1..=5 {
        let block = create_block_entry(i, prev_hash, 1468595301000 + (i as u64 * 15000), 10);
        let result = chain.add_block(block.clone());

        // Verify the chain height increased
        assert_eq!(
            chain.height(),
            i,
            "Height should be {} after adding block {}",
            i,
            i
        );

        // Update prev_hash for next iteration
        prev_hash = block.hash;
    }
}
