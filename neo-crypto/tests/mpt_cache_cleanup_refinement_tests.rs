//! Behavioral regression tests for the global node cache implementation.
//! These exercise the Rust API; they are not extracted Coq proofs or a proof
//! of MptCache's persisted reference-count semantics.

use neo_crypto::mpt_trie::{cache::GlobalNodeCache, node::Node};
use neo_primitives::UInt256;
use std::num::NonZero;
use std::sync::Arc;

fn key(index: u64) -> UInt256 {
    let mut bytes = [0; 32];
    bytes[..8].copy_from_slice(&index.to_le_bytes());
    UInt256::from(bytes)
}

fn cache(capacity: usize) -> GlobalNodeCache {
    GlobalNodeCache::with_capacity(NonZero::new(capacity).unwrap())
}

#[test]
fn test_capacity_enforced() {
    const CAPACITY: usize = 1_000_000;
    let cache = GlobalNodeCache::new();
    let node = Arc::new(Node::new());
    for i in 0..CAPACITY + 500 {
        cache.put(key(i as u64), Arc::clone(&node));
        assert_eq!(cache.len(), (i + 1).min(CAPACITY));
    }
    for i in 0..500 {
        assert!(cache.get(&key(i)).is_none());
    }
    assert!(cache.get(&key(500)).is_some());
}

#[test]
fn test_lru_eviction_correct() {
    const CAPACITY: usize = 1000;
    let cache = cache(CAPACITY);
    for i in 0..=CAPACITY as u64 {
        cache.put(key(i), Arc::new(Node::new()));
    }
    assert_eq!(cache.len(), CAPACITY);
    assert!(cache.get(&key(0)).is_none());
    for i in 1..=CAPACITY as u64 {
        assert!(cache.get(&key(i)).is_some(), "Entry {i} must be retained");
    }
}

#[test]
fn test_ref_count_safe() {
    let cache = cache(1);
    let node = Arc::new(Node::new_branch());
    assert_eq!(Arc::strong_count(&node), 1);
    cache.put(key(0), Arc::clone(&node));
    assert_eq!(Arc::strong_count(&node), 2);
    let retrieved = cache.get(&key(0)).unwrap();
    assert_eq!(Arc::strong_count(&node), 3);
    cache.clear();
    assert_eq!(Arc::strong_count(&node), 2);
    drop(retrieved);
    assert_eq!(Arc::strong_count(&node), 1);
}

#[test]
fn test_multiple_operations_consistent() {
    let cache = cache(10);
    let node = Arc::new(Node::new_leaf(vec![1; 100]));
    cache.put(key(42), Arc::clone(&node));
    for _ in 0..10 {
        assert!(Arc::ptr_eq(&cache.get(&key(42)).unwrap(), &node));
    }
    assert_eq!(cache.len(), 1);
    assert_eq!(cache.stats(), (10, 0, 100.0));
}

#[test]
fn test_hit_rate_valid() {
    let cache = cache(100);
    for i in 0..100 {
        cache.put(key(i), Arc::new(Node::new()));
    }
    for i in 0..200 {
        assert_eq!(cache.get(&key(i)).is_some(), i < 100);
    }
    assert_eq!(cache.stats(), (100, 100, 50.0));
}

#[test]
fn test_eviction_preserves_consistency() {
    let cache = cache(50);
    let oldest = Arc::new(Node::new_leaf(vec![42]));
    let weak = Arc::downgrade(&oldest);
    cache.put(key(0), oldest);
    let held = cache.get(&key(0)).unwrap();
    for i in 1..=50 {
        cache.put(key(i), Arc::new(Node::new()));
    }
    assert_eq!(cache.len(), 50);
    assert!(cache.get(&key(0)).is_none());
    assert!(weak.upgrade().is_some());
    assert_eq!(held.try_hash().unwrap(), Node::new_leaf(vec![42]).try_hash().unwrap());
    drop(held);
    assert!(weak.upgrade().is_none());
}

#[test]
fn test_concurrent_read_safety() {
    let cache = Arc::new(cache(1000));
    for i in 0..1000 {
        cache.put(key(i), Arc::new(Node::new()));
    }
    let handles: Vec<_> = (0..5)
        .map(|_| {
            let cache = Arc::clone(&cache);
            std::thread::spawn(move || {
                for i in 0..1000 {
                    assert!(cache.get(&key(i)).is_some());
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    assert_eq!(cache.stats(), (5000, 0, 100.0));
    assert_eq!(cache.len(), 1000);
}

#[test]
fn test_clear_resets_state() {
    let cache = cache(50);
    for i in 0..50 {
        cache.put(key(i), Arc::new(Node::new()));
    }
    assert!(cache.get(&key(0)).is_some());
    assert!(cache.get(&key(99)).is_none());
    let stats = cache.stats();
    cache.clear();
    assert!(cache.is_empty());
    assert_eq!(cache.stats(), stats);
    for i in 0..50 {
        assert!(cache.get(&key(i)).is_none());
    }
}

#[test]
fn test_large_values_preserved() {
    let cache = cache(1);
    let node = Arc::new(Node::new_leaf(vec![0xAB; 10_000]));
    let expected_hash = node.try_hash().unwrap();
    cache.put(expected_hash, Arc::clone(&node));
    let retrieved = cache.get(&expected_hash).unwrap();
    assert!(Arc::ptr_eq(&node, &retrieved));
    assert_eq!(retrieved.try_hash().unwrap(), expected_hash);
}

#[test]
fn test_cache_hit_performance() {
    let cache = cache(1);
    let mut branch = Node::new_branch();
    for i in 0..17 {
        branch.set_child(i, Node::new_leaf(vec![i as u8; 100]));
    }
    let node = Arc::new(branch);
    let hash = node.try_hash().unwrap();
    cache.put(hash, Arc::clone(&node));
    // Pointer identity checks reuse without a machine-dependent timing gate.
    for _ in 0..10_000 {
        assert!(Arc::ptr_eq(&cache.get(&hash).unwrap(), &node));
    }
    assert_eq!(cache.stats(), (10_000, 0, 100.0));
}

#[test]
fn test_ref_count_no_overflow() {
    let cache = cache(1);
    let node = Arc::new(Node::new());
    for _ in 0..1000 {
        cache.put(key(0), Arc::clone(&node));
        assert_eq!(Arc::strong_count(&node), 2);
    }
    cache.clear();
    assert_eq!(Arc::strong_count(&node), 1);
}

#[test]
fn test_boundary_single_entry() {
    let cache = cache(1);
    cache.put(key(0), Arc::new(Node::new()));
    assert_eq!(cache.len(), 1);
    assert!(cache.get(&key(0)).is_some());
    cache.put(key(1), Arc::new(Node::new()));
    assert_eq!(cache.len(), 1);
    assert!(cache.get(&key(0)).is_none());
    assert!(cache.get(&key(1)).is_some());
}

#[test]
fn test_hash_diversity() {
    let cache = cache(1000);
    let keys: std::collections::HashSet<_> = (0..1000).map(key).collect();
    assert_eq!(keys.len(), 1000);
    for hash in &keys {
        cache.put(*hash, Arc::new(Node::new()));
    }
    assert_eq!(cache.len(), 1000);
    for hash in &keys {
        assert!(cache.get(hash).is_some());
    }
}

#[test]
fn test_cache_initially_empty() {
    let cache = cache(10);
    assert!(cache.is_empty());
    assert_eq!(cache.len(), 0);
    assert_eq!(cache.stats(), (0, 0, 0.0));
}

#[test]
fn test_insertion_order_affects_eviction() {
    let cache = cache(3);
    for i in 0..3 {
        cache.put(key(i), Arc::new(Node::new()));
    }
    assert!(cache.get(&key(0)).is_some());
    cache.put(key(3), Arc::new(Node::new()));
    assert!(cache.get(&key(1)).is_none());
    for i in [0, 2, 3] {
        assert!(cache.get(&key(i)).is_some());
    }
    let replacement = Arc::new(Node::new_leaf(vec![9]));
    cache.put(key(2), Arc::clone(&replacement));
    cache.put(key(4), Arc::new(Node::new()));
    assert!(cache.get(&key(0)).is_none());
    assert!(Arc::ptr_eq(&cache.get(&key(2)).unwrap(), &replacement));
    assert_eq!(cache.len(), 3);
}

#[test]
fn test_stats_accuracy_heavy_load() {
    let cache = cache(200);
    for i in 0..10_000 {
        let hash = key(i % 200);
        if i % 2 == 0 {
            cache.put(hash, Arc::new(Node::new()));
            assert!(cache.get(&hash).is_some());
        } else {
            assert!(cache.get(&hash).is_none());
        }
    }
    assert_eq!(cache.stats(), (5000, 5000, 50.0));
}

#[test]
fn test_memory_isolation() {
    let first = cache(100);
    let second = cache(100);
    first.put(key(0), Arc::new(Node::new()));
    assert!(first.get(&key(0)).is_some());
    assert!(second.is_empty());
    assert_eq!(second.stats(), (0, 0, 0.0));
    second.put(key(0), Arc::new(Node::new_leaf(vec![2])));
    first.clear();
    assert!(first.is_empty());
    assert!(second.get(&key(0)).is_some());
}

#[test]
fn test_complex_tree_structure() {
    let cache = cache(1);
    let mut branch = Node::new_branch();
    let leaf1 = Arc::new(Node::new_leaf(vec![1, 2, 3]));
    let leaf2 = Arc::new(Node::new_leaf(vec![4, 5, 6]));
    branch.children[0] = Arc::clone(&leaf1);
    branch.children[1] = Arc::clone(&leaf2);
    let hash = branch.try_hash().unwrap();
    cache.put(hash, Arc::new(branch));
    let retrieved = cache.get(&hash).unwrap();
    assert_eq!(retrieved.try_hash().unwrap(), hash);
    assert!(Arc::ptr_eq(&retrieved.children[0], &leaf1));
    assert!(Arc::ptr_eq(&retrieved.children[1], &leaf2));
}

#[test]
fn test_extreme_sequential_load() {
    let cache = cache(128);
    for i in 0..10_000 {
        cache.put(key(i), Arc::new(Node::new()));
        assert_eq!(cache.len(), (i as usize + 1).min(128));
    }
    for i in 0..10_000 {
        assert_eq!(cache.get(&key(i)).is_some(), i >= 10_000 - 128);
    }
}

#[test]
fn test_e2e_block_processing() {
    // This is a cache workload, not execution of the production block pipeline.
    let cache = cache(100);
    for i in 0..100 {
        cache.put(key(i), Arc::new(Node::new_branch()));
    }
    assert_eq!(cache.len(), 100);
    for i in 0..100 {
        assert!(cache.get(&key(i)).is_some());
    }
    for i in 0..10 {
        let updated = Arc::new(Node::new_leaf(vec![i as u8]));
        cache.put(key(i), Arc::clone(&updated));
        assert!(Arc::ptr_eq(&cache.get(&key(i)).unwrap(), &updated));
    }
    assert_eq!(cache.len(), 100);
    assert_eq!(cache.stats(), (110, 0, 100.0));
}
