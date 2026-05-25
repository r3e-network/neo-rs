use super::*;

#[test]
fn read_cache_put_and_get() {
    let cache = ReadCache::<String, String>::with_defaults();

    cache.put("key1".to_string(), "value1".to_string(), 10);
    cache.put("key2".to_string(), "value2".to_string(), 10);

    assert_eq!(cache.get(&"key1".to_string()), Some("value1".to_string()));
    assert_eq!(cache.get(&"key2".to_string()), Some("value2".to_string()));
    assert_eq!(cache.get(&"key3".to_string()), None);

    let stats = cache.stats();
    assert_eq!(stats.hits, 2);
    assert_eq!(stats.misses, 1);
}

#[test]
fn read_cache_eviction() {
    let config = ReadCacheConfig {
        max_entries: 2,
        max_bytes: 1000,
        enable_prefetch: false,
        prefetch_count: 0,
        prefetch_threshold: 0,
        ttl: None,
        enable_stats: true,
        enable_bloom_filter: false,
        bloom_filter_capacity: 1000,
        bloom_filter_fpr: 0.01,
    };

    let cache = ReadCache::<String, String>::new(config);

    cache.put("key1".to_string(), "value1".to_string(), 10);
    cache.put("key2".to_string(), "value2".to_string(), 10);
    cache.put("key3".to_string(), "value3".to_string(), 10); // Should evict key1

    assert_eq!(cache.len(), 2);
    assert_eq!(cache.get(&"key1".to_string()), None); // Evicted
    assert!(cache.get(&"key2".to_string()).is_some());
    assert!(cache.get(&"key3".to_string()).is_some());

    let stats = cache.stats();
    assert_eq!(stats.evictions, 1);
}

#[test]
fn read_cache_get_refreshes_lru_order() {
    let config = ReadCacheConfig {
        max_entries: 2,
        max_bytes: 1000,
        enable_prefetch: false,
        prefetch_count: 0,
        prefetch_threshold: 0,
        ttl: None,
        enable_stats: true,
        enable_bloom_filter: false,
        bloom_filter_capacity: 1000,
        bloom_filter_fpr: 0.01,
    };

    let cache = ReadCache::<String, String>::new(config);

    cache.put("key1".to_string(), "value1".to_string(), 10);
    cache.put("key2".to_string(), "value2".to_string(), 10);
    assert_eq!(cache.get(&"key1".to_string()), Some("value1".to_string()));

    cache.put("key3".to_string(), "value3".to_string(), 10);

    assert_eq!(cache.len(), 2);
    assert!(cache.contains(&"key1".to_string()));
    assert!(!cache.contains(&"key2".to_string()));
    assert!(cache.contains(&"key3".to_string()));
}

#[test]
fn read_cache_contains_does_not_refresh_lru_order() {
    let config = ReadCacheConfig {
        max_entries: 2,
        max_bytes: 1000,
        enable_prefetch: false,
        prefetch_count: 0,
        prefetch_threshold: 0,
        ttl: None,
        enable_stats: true,
        enable_bloom_filter: false,
        bloom_filter_capacity: 1000,
        bloom_filter_fpr: 0.01,
    };

    let cache = ReadCache::<String, String>::new(config);

    cache.put("key1".to_string(), "value1".to_string(), 10);
    cache.put("key2".to_string(), "value2".to_string(), 10);
    assert!(cache.contains(&"key1".to_string()));

    cache.put("key3".to_string(), "value3".to_string(), 10);

    assert_eq!(cache.len(), 2);
    assert!(!cache.contains(&"key1".to_string()));
    assert!(cache.contains(&"key2".to_string()));
    assert!(cache.contains(&"key3".to_string()));
}

#[test]
fn read_cache_should_prefetch_does_not_refresh_lru_order() {
    let config = ReadCacheConfig {
        max_entries: 2,
        max_bytes: 1000,
        enable_prefetch: true,
        prefetch_count: 5,
        prefetch_threshold: 1,
        ttl: None,
        enable_stats: true,
        enable_bloom_filter: false,
        bloom_filter_capacity: 1000,
        bloom_filter_fpr: 0.01,
    };

    let cache = ReadCache::<String, String>::new(config);

    cache.put("key1".to_string(), "value1".to_string(), 10);
    cache.put("key2".to_string(), "value2".to_string(), 10);
    assert!(cache.should_prefetch(&"key1".to_string()));

    cache.put("key3".to_string(), "value3".to_string(), 10);

    assert_eq!(cache.len(), 2);
    assert!(!cache.contains(&"key1".to_string()));
    assert!(cache.contains(&"key2".to_string()));
    assert!(cache.contains(&"key3".to_string()));
}

#[test]
fn read_cache_remove_clears_access_order_entry() {
    let config = ReadCacheConfig {
        max_entries: 2,
        max_bytes: 1000,
        enable_prefetch: false,
        prefetch_count: 0,
        prefetch_threshold: 0,
        ttl: None,
        enable_stats: true,
        enable_bloom_filter: false,
        bloom_filter_capacity: 1000,
        bloom_filter_fpr: 0.01,
    };

    let cache = ReadCache::<String, String>::new(config);

    cache.put("key1".to_string(), "value1".to_string(), 10);
    cache.put("key2".to_string(), "value2".to_string(), 10);
    assert_eq!(
        cache.remove(&"key1".to_string()),
        Some("value1".to_string())
    );

    cache.put("key3".to_string(), "value3".to_string(), 10);
    cache.put("key4".to_string(), "value4".to_string(), 10);

    assert_eq!(cache.len(), 2);
    assert!(!cache.contains(&"key1".to_string()));
    assert!(!cache.contains(&"key2".to_string()));
    assert!(cache.contains(&"key3".to_string()));
    assert!(cache.contains(&"key4".to_string()));
}

#[test]
fn read_cache_update_existing_entry_does_not_evict_unrelated_key() {
    let config = ReadCacheConfig {
        max_entries: 2,
        max_bytes: 1000,
        enable_prefetch: false,
        prefetch_count: 0,
        prefetch_threshold: 0,
        ttl: None,
        enable_stats: true,
        enable_bloom_filter: false,
        bloom_filter_capacity: 1000,
        bloom_filter_fpr: 0.01,
    };

    let cache = ReadCache::<String, String>::new(config);

    cache.put("key1".to_string(), "value1".to_string(), 10);
    cache.put("key2".to_string(), "value2".to_string(), 10);
    cache.put("key2".to_string(), "value2b".to_string(), 30);

    assert_eq!(cache.len(), 2);
    assert_eq!(cache.get(&"key1".to_string()), Some("value1".to_string()));
    assert_eq!(cache.get(&"key2".to_string()), Some("value2b".to_string()));

    let stats = cache.stats();
    assert_eq!(stats.evictions, 0);
    assert_eq!(stats.current_entries, 2);
    assert_eq!(stats.current_bytes, 40);
}

#[test]
fn read_cache_byte_limit_recomputes_after_each_eviction() {
    let config = ReadCacheConfig {
        max_entries: 100,
        max_bytes: 35,
        enable_prefetch: false,
        prefetch_count: 0,
        prefetch_threshold: 0,
        ttl: None,
        enable_stats: true,
        enable_bloom_filter: false,
        bloom_filter_capacity: 1000,
        bloom_filter_fpr: 0.01,
    };

    let cache = ReadCache::<String, String>::new(config);

    cache.put("key1".to_string(), "value1".to_string(), 10);
    cache.put("key2".to_string(), "value2".to_string(), 10);
    cache.put("key3".to_string(), "value3".to_string(), 20);

    assert_eq!(cache.len(), 2);
    assert!(!cache.contains(&"key1".to_string()));
    assert!(cache.contains(&"key2".to_string()));
    assert!(cache.contains(&"key3".to_string()));

    let stats = cache.stats();
    assert_eq!(stats.evictions, 1);
    assert_eq!(stats.current_entries, 2);
    assert_eq!(stats.current_bytes, 30);
}

#[test]
fn read_cache_byte_limit_still_applies_when_stats_are_disabled() {
    let config = ReadCacheConfig {
        max_entries: 100,
        max_bytes: 25,
        enable_prefetch: false,
        prefetch_count: 0,
        prefetch_threshold: 0,
        ttl: None,
        enable_stats: false,
        enable_bloom_filter: false,
        bloom_filter_capacity: 1000,
        bloom_filter_fpr: 0.01,
    };

    let cache = ReadCache::<String, String>::new(config);

    cache.put("key1".to_string(), "value1".to_string(), 20);
    cache.put("key2".to_string(), "value2".to_string(), 20);

    assert_eq!(cache.len(), 1);
    assert!(!cache.contains(&"key1".to_string()));
    assert!(cache.contains(&"key2".to_string()));

    let stats = cache.stats();
    assert_eq!(stats.evictions, 0);
    assert_eq!(stats.inserts, 0);
    assert_eq!(stats.current_entries, 1);
    assert_eq!(stats.current_bytes, 20);
}

#[test]
fn read_cache_put_batch_update_does_not_evict_unrelated_key() {
    let config = ReadCacheConfig {
        max_entries: 2,
        max_bytes: 1000,
        enable_prefetch: false,
        prefetch_count: 0,
        prefetch_threshold: 0,
        ttl: None,
        enable_stats: true,
        enable_bloom_filter: false,
        bloom_filter_capacity: 1000,
        bloom_filter_fpr: 0.01,
    };

    let cache = ReadCache::<String, String>::new(config);

    cache.put("key1".to_string(), "value1".to_string(), 10);
    cache.put("key2".to_string(), "value2".to_string(), 10);
    cache.put_batch(vec![("key2".to_string(), "value2b".to_string(), 30)]);

    assert_eq!(cache.len(), 2);
    assert_eq!(cache.get(&"key1".to_string()), Some("value1".to_string()));
    assert_eq!(cache.get(&"key2".to_string()), Some("value2b".to_string()));

    let stats = cache.stats();
    assert_eq!(stats.evictions, 0);
    assert_eq!(stats.prefetches, 1);
    assert_eq!(stats.current_entries, 2);
    assert_eq!(stats.current_bytes, 40);
}

#[test]
fn read_cache_put_batch_duplicate_key_accounts_final_value_once() {
    let config = ReadCacheConfig {
        max_entries: 100,
        max_bytes: 1000,
        enable_prefetch: false,
        prefetch_count: 0,
        prefetch_threshold: 0,
        ttl: None,
        enable_stats: true,
        enable_bloom_filter: false,
        bloom_filter_capacity: 1000,
        bloom_filter_fpr: 0.01,
    };

    let cache = ReadCache::<String, String>::new(config);

    cache.put_batch(vec![
        ("key1".to_string(), "value1".to_string(), 10),
        ("key1".to_string(), "value1b".to_string(), 30),
        ("key2".to_string(), "value2".to_string(), 5),
    ]);

    assert_eq!(cache.len(), 2);
    assert_eq!(cache.get(&"key1".to_string()), Some("value1b".to_string()));
    assert_eq!(cache.get(&"key2".to_string()), Some("value2".to_string()));

    let stats = cache.stats();
    assert_eq!(stats.prefetches, 3);
    assert_eq!(stats.inserts, 3);
    assert_eq!(stats.current_entries, 2);
    assert_eq!(stats.current_bytes, 35);
}

#[test]
fn read_cache_byte_limit_eviction() {
    let config = ReadCacheConfig {
        max_entries: 100,
        max_bytes: 30,
        enable_prefetch: false,
        prefetch_count: 0,
        prefetch_threshold: 0,
        ttl: None,
        enable_stats: true,
        enable_bloom_filter: false,
        bloom_filter_capacity: 1000,
        bloom_filter_fpr: 0.01,
    };

    let cache = ReadCache::<String, String>::new(config);

    cache.put("key1".to_string(), "value1".to_string(), 20);
    cache.put("key2".to_string(), "value2".to_string(), 20); // Should trigger eviction

    // Should have evicted to make room
    assert!(cache.len() <= 2);
}

#[test]
fn read_cache_ttl_expiration() {
    let config = ReadCacheConfig {
        max_entries: 100,
        max_bytes: 1000,
        enable_prefetch: false,
        prefetch_count: 0,
        prefetch_threshold: 0,
        ttl: Some(Duration::from_millis(1)),
        enable_stats: true,
        enable_bloom_filter: false,
        bloom_filter_capacity: 1000,
        bloom_filter_fpr: 0.01,
    };

    let cache = ReadCache::<String, String>::new(config);

    cache.put("key1".to_string(), "value1".to_string(), 10);

    // Should be available immediately
    assert!(cache.get(&"key1".to_string()).is_some());

    // Wait for expiration
    std::thread::sleep(Duration::from_millis(10));

    // Should be expired now
    assert_eq!(cache.get(&"key1".to_string()), None);
}

#[test]
fn read_cache_ttl_expiration_updates_cache_accounting() {
    let config = ReadCacheConfig {
        max_entries: 100,
        max_bytes: 1000,
        enable_prefetch: false,
        prefetch_count: 0,
        prefetch_threshold: 0,
        ttl: Some(Duration::from_millis(1)),
        enable_stats: true,
        enable_bloom_filter: false,
        bloom_filter_capacity: 1000,
        bloom_filter_fpr: 0.01,
    };

    let cache = ReadCache::<String, String>::new(config);

    cache.put("key1".to_string(), "value1".to_string(), 10);
    std::thread::sleep(Duration::from_millis(10));

    assert_eq!(cache.get(&"key1".to_string()), None);

    let stats = cache.stats();
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.evictions, 1);
    assert_eq!(stats.current_entries, 0);
    assert_eq!(stats.current_bytes, 0);
}

#[test]
fn read_cache_contains_does_not_expire_ttl_entry() {
    let config = ReadCacheConfig {
        max_entries: 100,
        max_bytes: 1000,
        enable_prefetch: false,
        prefetch_count: 0,
        prefetch_threshold: 0,
        ttl: Some(Duration::from_millis(1)),
        enable_stats: true,
        enable_bloom_filter: false,
        bloom_filter_capacity: 1000,
        bloom_filter_fpr: 0.01,
    };

    let cache = ReadCache::<String, String>::new(config);
    let key = "key1".to_string();

    cache.put(key.clone(), "value1".to_string(), 10);
    std::thread::sleep(Duration::from_millis(10));

    assert!(cache.contains(&key));
    let stats = cache.stats();
    assert_eq!(stats.current_entries, 1);
    assert_eq!(stats.current_bytes, 10);
    assert_eq!(stats.misses, 0);
    assert_eq!(stats.evictions, 0);

    assert_eq!(cache.get(&key), None);
    let stats = cache.stats();
    assert_eq!(stats.current_entries, 0);
    assert_eq!(stats.current_bytes, 0);
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.evictions, 1);
}

#[test]
fn read_cache_remove() {
    let cache = ReadCache::<String, String>::with_defaults();

    cache.put("key1".to_string(), "value1".to_string(), 10);

    let removed = cache.remove(&"key1".to_string());
    assert_eq!(removed, Some("value1".to_string()));
    assert_eq!(cache.get(&"key1".to_string()), None);
}

#[test]
fn read_cache_remove_updates_occupancy_without_hit_miss_or_eviction() {
    let cache = ReadCache::<String, String>::with_defaults();

    cache.put("key1".to_string(), "value1".to_string(), 10);
    cache.put("key2".to_string(), "value2".to_string(), 20);

    assert_eq!(
        cache.remove(&"key1".to_string()),
        Some("value1".to_string())
    );

    let stats = cache.stats();
    assert_eq!(stats.current_entries, 1);
    assert_eq!(stats.current_bytes, 20);
    assert_eq!(stats.hits, 0);
    assert_eq!(stats.misses, 0);
    assert_eq!(stats.evictions, 0);
    assert_eq!(stats.inserts, 2);
}

#[test]
fn read_cache_clear() {
    let cache = ReadCache::<String, String>::with_defaults();

    cache.put("key1".to_string(), "value1".to_string(), 10);
    cache.put("key2".to_string(), "value2".to_string(), 10);

    cache.clear();

    assert!(cache.is_empty());
    assert_eq!(cache.get(&"key1".to_string()), None);
}

#[test]
fn read_cache_clear_preserves_history_counters_and_zeroes_occupancy() {
    let cache = ReadCache::<String, String>::with_defaults();

    cache.put("key1".to_string(), "value1".to_string(), 10);
    cache.put("key2".to_string(), "value2".to_string(), 20);
    assert_eq!(cache.get(&"key1".to_string()), Some("value1".to_string()));
    assert_eq!(cache.get(&"missing".to_string()), None);

    cache.clear();

    let stats = cache.stats();
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.inserts, 2);
    assert_eq!(stats.current_entries, 0);
    assert_eq!(stats.current_bytes, 0);
    assert_eq!(cache.len(), 0);
}

#[test]
fn read_cache_put_batch() {
    let cache = ReadCache::<String, String>::with_defaults();

    let items = vec![
        ("key1".to_string(), "value1".to_string(), 10),
        ("key2".to_string(), "value2".to_string(), 10),
        ("key3".to_string(), "value3".to_string(), 10),
    ];

    cache.put_batch(items);

    assert_eq!(cache.len(), 3);

    let stats = cache.stats();
    assert_eq!(stats.prefetches, 3);
}

#[test]
fn read_cache_stats_disabled_keeps_exact_occupancy_for_updates_batches_and_remove() {
    let config = ReadCacheConfig {
        max_entries: 100,
        max_bytes: 1000,
        enable_prefetch: false,
        prefetch_count: 0,
        prefetch_threshold: 0,
        ttl: None,
        enable_stats: false,
        enable_bloom_filter: false,
        bloom_filter_capacity: 1000,
        bloom_filter_fpr: 0.01,
    };

    let cache = ReadCache::<String, String>::new(config);

    cache.put("key1".to_string(), "value1".to_string(), 10);
    cache.put("key2".to_string(), "value2".to_string(), 20);
    cache.put("key1".to_string(), "value1b".to_string(), 15);
    cache.put_batch(vec![
        ("key2".to_string(), "value2b".to_string(), 5),
        ("key3".to_string(), "value3".to_string(), 30),
    ]);

    assert_eq!(
        cache.remove(&"key1".to_string()),
        Some("value1b".to_string())
    );

    let stats = cache.stats();
    assert_eq!(stats.inserts, 0);
    assert_eq!(stats.prefetches, 0);
    assert_eq!(stats.evictions, 0);
    assert_eq!(stats.current_entries, 2);
    assert_eq!(stats.current_bytes, 35);
    assert_eq!(cache.get(&"key2".to_string()), Some("value2b".to_string()));
    assert_eq!(cache.get(&"key3".to_string()), Some("value3".to_string()));
}

#[test]
fn read_cache_keeps_single_entry_larger_than_byte_limit() {
    let config = ReadCacheConfig {
        max_entries: 100,
        max_bytes: 10,
        enable_prefetch: false,
        prefetch_count: 0,
        prefetch_threshold: 0,
        ttl: None,
        enable_stats: true,
        enable_bloom_filter: false,
        bloom_filter_capacity: 1000,
        bloom_filter_fpr: 0.01,
    };

    let cache = ReadCache::<String, String>::new(config);

    cache.put("oversize".to_string(), "value".to_string(), 20);

    assert_eq!(
        cache.get(&"oversize".to_string()),
        Some("value".to_string())
    );
    let stats = cache.stats();
    assert_eq!(stats.current_entries, 1);
    assert_eq!(stats.current_bytes, 20);
    assert_eq!(stats.evictions, 0);
}

#[test]
fn read_cache_should_prefetch() {
    let config = ReadCacheConfig {
        max_entries: 100,
        max_bytes: 1000,
        enable_prefetch: true,
        prefetch_count: 5,
        prefetch_threshold: 2, // Need 2 accesses to trigger prefetch
        ttl: None,
        enable_stats: true,
        enable_bloom_filter: false,
        bloom_filter_capacity: 1000,
        bloom_filter_fpr: 0.01,
    };

    let cache = ReadCache::<String, String>::new(config);

    // put() initializes access_count to 1
    cache.put("key1".to_string(), "value1".to_string(), 10);

    // After put, access_count = 1, should not prefetch
    assert!(!cache.should_prefetch(&"key1".to_string()));

    // First get increments to 2, now meets threshold
    cache.get(&"key1".to_string());

    // After first get, access_count = 2, should prefetch
    assert!(cache.should_prefetch(&"key1".to_string()));
}

#[test]
fn read_cache_stats_hit_rate() {
    let stats = ReadCacheStatsSnapshot {
        hits: 75,
        misses: 25,
        evictions: 0,
        prefetches: 0,
        prefetch_hits: 0,
        inserts: 0,
        current_entries: 0,
        current_bytes: 0,
        bloom_filter_negatives: 0,
        bloom_filter_checks: 0,
    };

    assert!((stats.hit_rate() - 0.75).abs() < 0.001);
}

#[test]
fn bloom_filter_basic_operations() {
    let bloom = NegativeLookupBloom::new(1000, 0.01);

    // Check non-existent key
    assert!(!bloom.might_contain_bytes(b"key1"));

    // Insert key
    bloom.insert_bytes(b"key1");

    // Should now report might contain
    assert!(bloom.might_contain_bytes(b"key1"));

    // Check count
    assert_eq!(bloom.len(), 1);

    // Clear and verify
    bloom.clear();
    assert!(!bloom.might_contain_bytes(b"key1"));
    assert!(bloom.is_empty());
}

#[test]
fn bloom_filter_prehashed_operations_have_no_false_negatives() {
    let bloom = NegativeLookupBloom::new(16, 0.01);
    let hash = xxhash_rust::xxh3::xxh3_64(b"prehashed-key");

    assert!(!bloom.might_contain_hash(hash));

    bloom.insert_hash(hash);

    assert!(bloom.might_contain_hash(hash));
    assert_eq!(bloom.len(), 1);
}

#[test]
fn bloom_filter_byte_and_hash_paths_share_source_hash() {
    let key = b"cross-path-key";
    let hash = xxhash_rust::xxh3::xxh3_64(key);

    let bytes_first = NegativeLookupBloom::new(16, 0.01);
    bytes_first.insert_bytes(key);
    assert!(bytes_first.might_contain_hash(hash));

    let hash_first = NegativeLookupBloom::new(16, 0.01);
    hash_first.insert_hash(hash);
    assert!(hash_first.might_contain_bytes(key));
}

#[test]
fn bloom_filter_should_rebuild_at_capacity() {
    let bloom = NegativeLookupBloom::new(2, 0.01);

    assert!(!bloom.should_rebuild());

    bloom.insert_hash(1);
    assert!(!bloom.should_rebuild());

    bloom.insert_hash(2);
    assert!(bloom.should_rebuild());
}

#[test]
fn bloom_filter_accepts_boundary_configuration_without_panicking() {
    let bloom = NegativeLookupBloom::new(0, 0.0);

    assert!(bloom.is_empty());
    bloom.insert_bytes(b"key1");

    assert!(bloom.might_contain_bytes(b"key1"));
    assert_eq!(bloom.len(), 1);
    assert!(bloom.should_rebuild());
}

#[test]
fn bloom_filter_concurrent_insert_has_no_false_negatives() {
    let bloom = Arc::new(NegativeLookupBloom::new(1024, 0.01));
    let mut handles = Vec::new();

    for shard in 0..4 {
        let bloom = Arc::clone(&bloom);
        handles.push(std::thread::spawn(move || {
            for offset in 0..250 {
                bloom.insert_hash((shard * 1_000 + offset) as u64);
            }
        }));
    }

    for handle in handles {
        handle.join().expect("bloom insert thread panicked");
    }

    for shard in 0..4 {
        for offset in 0..250 {
            assert!(bloom.might_contain_hash((shard * 1_000 + offset) as u64));
        }
    }

    assert_eq!(bloom.len(), 1000);
}

#[test]
fn bloom_filter_negative_lookup_in_cache() {
    let config = ReadCacheConfig {
        max_entries: 100,
        max_bytes: 1000,
        enable_prefetch: false,
        prefetch_count: 0,
        prefetch_threshold: 0,
        ttl: None,
        enable_stats: true,
        enable_bloom_filter: true,
        bloom_filter_capacity: 1000,
        bloom_filter_fpr: 0.01,
    };

    let cache = ReadCache::<String, String>::new(config);

    // Insert a key
    cache.put("existing".to_string(), "value".to_string(), 10);

    // Check non-existent key - should use bloom filter for fast negative
    let result = cache.get(&"nonexistent".to_string());
    assert!(result.is_none());

    // Existing key should work
    assert_eq!(
        cache.get(&"existing".to_string()),
        Some("value".to_string())
    );

    let stats = cache.stats();
    assert!(stats.bloom_filter_checks > 0);
}

#[test]
fn storage_key_bloom_hash_matches_id_little_endian_plus_key_bytes() {
    use crate::smart_contract::StorageKey;

    let key = StorageKey::new(-42, b"contract-storage-key".to_vec());
    let mut expected = xxhash_rust::xxh3::Xxh3::new();
    expected.update(&(-42i32).to_le_bytes());
    expected.update(b"contract-storage-key");

    assert_eq!(key.hash_for_bloom(), expected.digest());
}

#[test]
fn read_cache_bloom_positive_after_remove_still_returns_map_miss() {
    let cache = ReadCache::<String, String>::with_defaults();
    let key = "key1".to_string();

    cache.put(key.clone(), "value1".to_string(), 10);
    assert!(cache.fast_bloom_check(&key));
    assert_eq!(cache.remove(&key), Some("value1".to_string()));

    assert!(cache.fast_bloom_check(&key));
    assert_eq!(cache.get(&key), None);
    assert!(!cache.contains(&key));
}

#[test]
fn read_cache_bloom_positive_after_eviction_still_returns_cache_miss() {
    let config = ReadCacheConfig {
        max_entries: 1,
        max_bytes: 1000,
        enable_prefetch: false,
        prefetch_count: 0,
        prefetch_threshold: 0,
        ttl: None,
        enable_stats: true,
        enable_bloom_filter: true,
        bloom_filter_capacity: 1000,
        bloom_filter_fpr: 0.01,
    };
    let cache = ReadCache::<String, String>::new(config);
    let evicted_key = "key1".to_string();

    cache.put(evicted_key.clone(), "value1".to_string(), 10);
    assert!(cache.fast_bloom_check(&evicted_key));

    cache.put("key2".to_string(), "value2".to_string(), 10);

    assert!(cache.fast_bloom_check(&evicted_key));
    assert_eq!(cache.get(&evicted_key), None);
    assert!(!cache.contains(&evicted_key));
}

#[test]
fn read_cache_with_storage_key() {
    use crate::smart_contract::StorageKey;

    let cache = ReadCache::<StorageKey, String>::with_defaults();

    let key1 = StorageKey::new(1, b"test1".to_vec());
    let key2 = StorageKey::new(2, b"test2".to_vec());

    cache.put(key1.clone(), "value1".to_string(), 20);
    cache.put(key2.clone(), "value2".to_string(), 20);

    assert_eq!(cache.get(&key1), Some("value1".to_string()));
    assert_eq!(cache.get(&key2), Some("value2".to_string()));

    let stats = cache.stats();
    assert_eq!(stats.hits, 2);
}

#[test]
fn read_cache_storage_key_put_batch_has_no_bloom_false_negatives() {
    use crate::smart_contract::StorageKey;

    let cache = ReadCache::<StorageKey, String>::with_defaults();
    let keys = [
        StorageKey::new(1, b"alpha".to_vec()),
        StorageKey::new(1, b"beta".to_vec()),
        StorageKey::new(2, b"alpha".to_vec()),
    ];

    cache.put_batch(
        keys.iter()
            .enumerate()
            .map(|(index, key)| (key.clone(), format!("value{index}"), 20))
            .collect(),
    );

    for (index, key) in keys.iter().enumerate() {
        assert!(cache.fast_bloom_check(key));
        assert_eq!(cache.get(key), Some(format!("value{index}")));
    }
}
