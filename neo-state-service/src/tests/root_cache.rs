use super::*;

fn sample_root(index: u32) -> StateRoot {
    StateRoot::new_current(index, UInt256::from([index as u8; 32]))
}

#[test]
fn cache_hits_and_misses() {
    let cache = StateRootCache::with_capacity(4);
    assert!(cache.get(&UInt256::from([1u8; 32])).is_none());
    cache.insert(sample_root(1));
    let hit = cache.get(&UInt256::from([1u8; 32]));
    assert!(hit.is_some());
    let snap = cache.stats().snapshot();
    assert_eq!(snap.misses, 1);
    assert_eq!(snap.hits, 1);
}

#[test]
fn cache_evicts_when_full() {
    let cache = StateRootCache::with_capacity(2);
    cache.insert(sample_root(1));
    cache.insert(sample_root(2));
    cache.insert(sample_root(3));
    // Cache capacity is 2; the oldest entry (root 1) should be evicted.
    assert!(cache.get(&UInt256::from([1u8; 32])).is_none());
    assert!(cache.get(&UInt256::from([2u8; 32])).is_some());
    assert!(cache.get(&UInt256::from([3u8; 32])).is_some());
}
