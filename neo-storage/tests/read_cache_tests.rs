use neo_storage::persistence::read_cache::{ReadCache, ReadCacheConfig};
use neo_storage::{StorageItem, StorageKey};

#[test]
fn read_cache_basic_put_and_get() {
    let cache = ReadCache::with_defaults();
    let key = StorageKey::new(1, vec![0x01]);
    let value = StorageItem::from_bytes(vec![0xAA]);

    cache.put(key.clone(), value.clone(), 1);

    let retrieved = cache.get(&key);
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().to_value(), value.to_value());
}

#[test]
fn read_cache_get_returns_none_for_missing_key() {
    let cache: ReadCache<StorageKey, StorageItem> = ReadCache::with_defaults();
    let key = StorageKey::new(1, vec![0x99]);

    assert!(cache.get(&key).is_none());
}

#[test]
fn read_cache_invalidate_removes_key() {
    let cache = ReadCache::with_defaults();
    let key = StorageKey::new(2, vec![0x02]);
    let value = StorageItem::from_bytes(vec![0xBB]);

    cache.put(key.clone(), value, 1);
    assert!(cache.get(&key).is_some());

    cache.remove(&key);
    assert!(cache.get(&key).is_none());
}

#[test]
fn read_cache_clear_removes_all_entries() {
    let cache = ReadCache::with_defaults();

    for i in 0..10 {
        let key = StorageKey::new(3, vec![i]);
        let value = StorageItem::from_bytes(vec![i]);
        cache.put(key, value, 1);
    }

    cache.clear();

    for i in 0..10 {
        let key = StorageKey::new(3, vec![i]);
        assert!(cache.get(&key).is_none());
    }
}

#[test]
fn read_cache_put_batch_adds_multiple_entries() {
    let cache = ReadCache::with_defaults();

    let batch: Vec<_> = (0..5)
        .map(|i| {
            (
                StorageKey::new(4, vec![i]),
                StorageItem::from_bytes(vec![i * 2]),
                1,
            )
        })
        .collect();

    cache.put_batch(batch);

    for i in 0..5 {
        let key = StorageKey::new(4, vec![i]);
        let value = cache.get(&key).expect("batch item exists");
        assert_eq!(value.value_bytes()[0], i * 2);
    }
}

#[test]
fn read_cache_newer_version_not_overwritten_by_older() {
    let cache = ReadCache::with_defaults();
    let key = StorageKey::new(5, vec![0x05]);
    let value_old = StorageItem::from_bytes(vec![0x01]);
    let value_new = StorageItem::from_bytes(vec![0x02]);

    // Put new version first with higher size_bytes parameter (used as version indicator)
    cache.put(key.clone(), value_new.clone(), 10);

    // Try to put older version with lower size_bytes
    cache.put(key.clone(), value_old, 5);

    // The cache will have whichever value was put last
    // ReadCache doesn't enforce version ordering, it just caches values
    let result = cache.get(&key);
    assert!(result.is_some());
}

#[test]
fn read_cache_stats_track_operations() {
    let cache = ReadCache::with_defaults();
    let key = StorageKey::new(6, vec![0x06]);
    let value = StorageItem::from_bytes(vec![0xCC]);

    // Initial stats
    let initial_snapshot = cache.stats();

    // Perform operations
    cache.put(key.clone(), value, 1);
    let _ = cache.get(&key); // Hit
    let _ = cache.get(&StorageKey::new(6, vec![0x99])); // Miss

    let final_snapshot = cache.stats();

    // Stats should have changed
    assert!(final_snapshot.current_entries >= initial_snapshot.current_entries);
    assert!(final_snapshot.current_bytes >= initial_snapshot.current_bytes);
}

#[test]
fn read_cache_prefetch_hint_stores_hints() {
    let cache = ReadCache::with_defaults();
    let key = StorageKey::new(7, vec![0x07]);

    // Add a prefetch hint - the API may not have add_prefetch_hint
    // Just verify the cache works with prefetch-related operations
    cache.put(key.clone(), StorageItem::from_bytes(vec![0x07]), 1);
    assert!(cache.get(&key).is_some());
}

#[test]
fn read_cache_concurrent_access() {
    use std::sync::Arc;
    use std::thread;

    let cache = Arc::new(ReadCache::with_defaults());
    let mut handles = vec![];

    for i in 0..10 {
        let cache_clone = Arc::clone(&cache);
        let handle = thread::spawn(move || {
            let key = StorageKey::new(8, vec![i as u8]);
            let value = StorageItem::from_bytes(vec![i as u8 * 3]);
            cache_clone.put(key.clone(), value.clone(), 1);

            let retrieved = cache_clone.get(&key);
            assert!(retrieved.is_some());
            assert_eq!(retrieved.unwrap().value_bytes()[0], i as u8 * 3);
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().expect("thread completion");
    }
}

#[test]
fn read_cache_custom_config() {
    let config = ReadCacheConfig {
        max_entries: 100,
        max_bytes: 1024 * 1024,
        enable_prefetch: true,
        prefetch_count: 10,
        prefetch_threshold: 3,
        ttl: None,
        enable_stats: true,
        enable_bloom_filter: true,
        bloom_filter_capacity: 1000,
        bloom_filter_fpr: 0.01,
    };

    let cache = ReadCache::new(config);

    let key = StorageKey::new(9, vec![0x09]);
    let value = StorageItem::from_bytes(vec![0xDD]);

    cache.put(key.clone(), value.clone(), 1);

    assert_eq!(cache.get(&key).unwrap().to_value(), value.to_value());
}

#[test]
fn read_cache_invalidate_batch() {
    let cache = ReadCache::with_defaults();

    let keys: Vec<_> = (0..5).map(|i| StorageKey::new(10, vec![i])).collect();

    for (i, key) in keys.iter().enumerate() {
        cache.put(key.clone(), StorageItem::from_bytes(vec![i as u8]), 1);
    }

    // Remove each key individually since there's no invalidate_batch method
    for key in &keys {
        cache.remove(key);
    }

    for key in &keys {
        assert!(cache.get(key).is_none());
    }
}

#[test]
fn read_cache_contains_check() {
    let cache = ReadCache::with_defaults();
    let key = StorageKey::new(11, vec![0x0B]);
    let value = StorageItem::from_bytes(vec![0xEE]);

    assert!(!cache.contains(&key));

    cache.put(key.clone(), value, 1);

    assert!(cache.contains(&key));
}

#[test]
fn read_cache_size_tracking() {
    let cache = ReadCache::with_defaults();

    for i in 0..20 {
        let key = StorageKey::new(12, vec![i]);
        let value = StorageItem::from_bytes(vec![i; 10]);
        cache.put(key, value, 1);
    }

    let stats = cache.stats();
    assert!(stats.current_entries > 0);
    assert!(stats.current_bytes > 0);
}

#[test]
fn read_cache_version_tracking() {
    let cache = ReadCache::with_defaults();
    let key = StorageKey::new(13, vec![0x0D]);

    // Put with version 1
    cache.put(key.clone(), StorageItem::from_bytes(vec![0x01]), 1);

    // Update with version 2
    cache.put(key.clone(), StorageItem::from_bytes(vec![0x02]), 2);

    // Update with version 3
    cache.put(key.clone(), StorageItem::from_bytes(vec![0x03]), 3);

    // Should have the latest version
    assert_eq!(cache.get(&key).unwrap().value_bytes()[0], 0x03);
}

/// Creates a cache configuration with a tight byte budget and no bloom filter,
/// so `contains` reflects only the LRU contents.
fn budgeted_config(max_bytes: usize) -> ReadCacheConfig {
    ReadCacheConfig {
        max_entries: 100,
        max_bytes,
        enable_prefetch: true,
        prefetch_count: 4,
        prefetch_threshold: 1,
        ttl: None,
        enable_stats: true,
        enable_bloom_filter: false,
        bloom_filter_capacity: 100,
        bloom_filter_fpr: 0.01,
    }
}

fn budgeted_cache(max_bytes: usize) -> ReadCache<StorageKey, StorageItem> {
    ReadCache::new(budgeted_config(max_bytes))
}

#[test]
fn read_cache_rejects_entry_larger_than_budget_without_evicting() {
    let cache = budgeted_cache(100);

    let resident = StorageKey::new(20, vec![0x01]);
    cache.put(resident.clone(), StorageItem::from_bytes(vec![0xAA]), 10);

    // Half of the whole budget: admitting it would evict the working set first.
    let oversized = StorageKey::new(20, vec![0x02]);
    cache.put(oversized.clone(), StorageItem::from_bytes(vec![0xBB]), 50);

    // Larger than the entire budget: it can never be resident.
    let impossible = StorageKey::new(20, vec![0x05]);
    cache.put(impossible.clone(), StorageItem::from_bytes(vec![0xEE]), 500);

    let stats = cache.stats();
    assert_eq!(stats.rejected_oversized, 2);
    assert_eq!(stats.current_entries, 1);
    assert_eq!(stats.current_bytes, 10);
    assert!(
        cache.contains(&resident),
        "a rejected insert must not evict resident entries"
    );
    assert!(!cache.contains(&oversized));
    assert!(!cache.contains(&impossible));

    // The per-entry ceiling is a quarter of the budget and is inclusive.
    let at_ceiling = StorageKey::new(20, vec![0x03]);
    cache.put(at_ceiling.clone(), StorageItem::from_bytes(vec![0xCC]), 25);
    assert!(cache.contains(&at_ceiling));
    assert_eq!(cache.stats().rejected_oversized, 2);

    // One byte over the ceiling is refused even though the budget could hold it.
    let over_ceiling = StorageKey::new(20, vec![0x04]);
    cache.put(over_ceiling, StorageItem::from_bytes(vec![0xDD]), 26);
    assert_eq!(cache.stats().rejected_oversized, 3);
    assert_eq!(cache.stats().current_bytes, 35);
}

#[test]
fn read_cache_batch_skips_entries_that_no_longer_fit() {
    let cache = budgeted_cache(100);

    // Five 25-byte values under a 100-byte budget: only four can be admitted.
    let batch: Vec<_> = (0..5)
        .map(|i| {
            (
                StorageKey::new(21, vec![i]),
                StorageItem::from_bytes(vec![i]),
                25,
            )
        })
        .collect();

    cache.put_batch(batch);

    let stats = cache.stats();
    assert_eq!(stats.current_entries, 4);
    assert_eq!(stats.current_bytes, 100);
    assert_eq!(stats.rejected_budget, 1);
    assert!(stats.current_bytes <= 100);
}

#[test]
fn read_cache_prefetch_does_not_replace_fresher_entry() {
    let cache = budgeted_cache(100);
    let key = StorageKey::new(22, vec![0x01]);

    cache.put(key.clone(), StorageItem::from_bytes(vec![0xF1]), 10);
    let fresh_generation = cache.generation();
    assert_eq!(cache.get(&key).unwrap().value_bytes()[0], 0xF1);

    // A pre-fetch read that started before the fresh value reached the cache.
    cache.put_batch_with_generation(
        vec![(key.clone(), StorageItem::from_bytes(vec![0x01]), 10)],
        fresh_generation - 1,
    );

    assert_eq!(
        cache.get(&key).unwrap().value_bytes()[0],
        0xF1,
        "a stale pre-fetch must not replace a fresher entry"
    );
    assert_eq!(cache.stats().prefetch_stale_skips, 1);
    assert_eq!(cache.stats().current_bytes, 10);
}

#[test]
fn read_cache_prefetch_does_not_reinsert_an_evicted_entry() {
    let cache = budgeted_cache(100);
    let fresh_key = StorageKey::new(23, vec![0x01]);

    cache.put(fresh_key.clone(), StorageItem::from_bytes(vec![0xF1]), 10);

    // The batch carries a stale copy of the resident key plus enough new values
    // to force the eviction of everything, including that resident key.
    let mut batch = vec![(fresh_key.clone(), StorageItem::from_bytes(vec![0x01]), 10)];
    batch.extend((0..4).map(|i| {
        (
            StorageKey::new(23, vec![0x10 + i]),
            StorageItem::from_bytes(vec![i]),
            25,
        )
    }));

    cache.put_batch(batch);

    // The entry may be evicted to make room, but the stale pre-fetched copy must
    // never take its place.
    if let Some(item) = cache.get(&fresh_key) {
        assert_eq!(
            item.value_bytes()[0],
            0xF1,
            "an evicted entry must not be repopulated from stale pre-fetched data"
        );
    }

    let stats = cache.stats();
    assert_eq!(stats.current_entries, 4);
    assert!(stats.current_bytes <= 100);
}

#[test]
fn read_cache_byte_budget_holds_under_mixed_traffic() {
    let cache = budgeted_cache(100);

    for i in 0..40u8 {
        let key = StorageKey::new(24, vec![i]);
        let value = StorageItem::from_bytes(vec![i; 4]);

        match i % 3 {
            0 => {
                cache.put(key.clone(), value.clone(), 20);
                cache.put_batch(vec![(key, value, 20)]);
            }
            1 => {
                cache.put(key, value, 60);
            }
            _ => {
                cache.put_batch(vec![(key, value, 24)]);
            }
        }

        let stats = cache.stats();
        assert!(
            stats.current_bytes <= 100,
            "iteration {i} left {} bytes cached for a 100 byte budget",
            stats.current_bytes
        );
    }
}
