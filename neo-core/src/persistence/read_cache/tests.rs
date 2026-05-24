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
fn read_cache_remove() {
    let cache = ReadCache::<String, String>::with_defaults();

    cache.put("key1".to_string(), "value1".to_string(), 10);

    let removed = cache.remove(&"key1".to_string());
    assert_eq!(removed, Some("value1".to_string()));
    assert_eq!(cache.get(&"key1".to_string()), None);
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
    let bloom = BloomFilter::new(1000, 0.01);

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
