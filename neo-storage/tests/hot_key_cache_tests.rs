//! Focused tests for the `DataCache`-level hot-key cache.
//!
//! The read cache and the hot-key cache both front the backing store, so these
//! tests disable the read cache wherever the hot-key cache is the behaviour
//! under test. Otherwise a read-cache hit would mask whether the hot-key layer
//! actually served the value.

use neo_storage::persistence::DataCache;
use neo_storage::persistence::data_cache::DataCacheConfig;
use neo_storage::{StorageItem, StorageKey};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

fn cache_config(enable_read_cache: bool) -> DataCacheConfig {
    DataCacheConfig {
        enable_read_cache,
        ..Default::default()
    }
}

/// Builds a cache whose backing store returns the raw key bytes as the value
/// and counts every physical read.
fn counting_cache(read_only: bool, config: DataCacheConfig) -> (DataCache, Arc<AtomicUsize>) {
    let reads = Arc::new(AtomicUsize::new(0));
    let store_reads = Arc::clone(&reads);
    let cache = DataCache::new_with_config(
        read_only,
        Some(Arc::new(move |key: &StorageKey| {
            store_reads.fetch_add(1, Ordering::Relaxed);
            Some(StorageItem::from_bytes(key.as_bytes().to_vec()))
        })),
        None,
        config,
    );
    (cache, reads)
}

fn key(suffix: u16) -> StorageKey {
    StorageKey::new(12, suffix.to_le_bytes().to_vec())
}

#[test]
fn committed_reads_are_served_by_the_hot_key_cache() {
    let (cache, reads) = counting_cache(true, cache_config(false));
    let target = key(1);

    assert!(cache.get(&target).is_some());
    assert!(cache.get(&target).is_some());

    assert_eq!(
        reads.load(Ordering::Relaxed),
        1,
        "the second read must not reach the backing store"
    );
    let stats = cache
        .hot_key_cache_stats()
        .expect("hot-key cache is enabled");
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.retained_entries, 1);
    assert_eq!(stats.capacity, 4096);
}

#[test]
fn disabling_the_hot_key_cache_falls_back_to_the_backing_store() {
    let config = DataCacheConfig {
        enable_hot_key_cache: false,
        enable_read_cache: false,
        // Without either read accelerator a read mirror would serve the second
        // lookup, so it is disabled to leave only the backing store.
        track_reads_in_write_cache: false,
        ..Default::default()
    };
    let (cache, reads) = counting_cache(true, config);
    let target = key(2);

    assert!(cache.get(&target).is_some());
    assert!(cache.get(&target).is_some());

    assert_eq!(reads.load(Ordering::Relaxed), 2);
    assert!(!cache.has_hot_key_cache());
    assert!(cache.hot_key_cache_stats().is_none());
}

#[test]
fn update_invalidates_the_hot_key_entry() {
    let (cache, _reads) = counting_cache(false, cache_config(false));
    let target = key(3);

    assert!(cache.get(&target).is_some());
    assert_eq!(
        cache
            .hot_key_cache_stats()
            .expect("hot-key cache is enabled")
            .retained_entries,
        1
    );

    cache.update(target.clone(), StorageItem::from_bytes(vec![0x99]));

    let stats = cache
        .hot_key_cache_stats()
        .expect("hot-key cache is enabled");
    assert_eq!(stats.invalidations, 1);
    assert_eq!(stats.retained_entries, 0);
    assert_eq!(cache.get(&target).unwrap().value_bytes().as_ref(), &[0x99]);
}

#[test]
fn delete_invalidates_the_hot_key_entry() {
    let (cache, _reads) = counting_cache(false, cache_config(false));
    let target = key(4);

    assert!(cache.get(&target).is_some());
    cache.delete(&target);

    let stats = cache
        .hot_key_cache_stats()
        .expect("hot-key cache is enabled");
    assert_eq!(stats.invalidations, 1);
    assert_eq!(stats.retained_entries, 0);
    assert!(cache.get(&target).is_none());
}

#[test]
fn hot_key_cache_never_exceeds_its_bound() {
    let config = DataCacheConfig {
        enable_read_cache: false,
        hot_key_cache_capacity: 8,
        ..Default::default()
    };
    let (cache, _reads) = counting_cache(true, config);

    for suffix in 0..128u16 {
        assert!(cache.get(&key(suffix)).is_some());
    }

    let stats = cache
        .hot_key_cache_stats()
        .expect("hot-key cache is enabled");
    assert_eq!(stats.capacity, 8);
    assert!(
        stats.retained_entries <= 8,
        "retained {} entries for an 8 entry bound",
        stats.retained_entries
    );
    assert!(stats.evictions > 0);
}

#[test]
fn overlays_run_without_a_hot_key_cache() {
    let (parent, reads) = counting_cache(false, cache_config(false));
    let overlay = parent.clone_cache();

    assert!(!overlay.has_hot_key_cache());
    assert!(overlay.hot_key_cache_stats().is_none());
    assert!(overlay.get(&key(5)).is_some());
    assert!(overlay.hot_key_cache_stats().is_none());

    // The overlay read still reached committed state through the parent.
    assert_eq!(reads.load(Ordering::Relaxed), 1);
}

#[test]
fn backing_store_generation_change_drops_hot_entries() {
    let generation = Arc::new(AtomicU64::new(0));
    let source_generation = Arc::clone(&generation);
    let (mut cache, reads) = counting_cache(false, cache_config(false));
    cache.set_generation_source(move || source_generation.load(Ordering::Acquire));
    let target = key(6);

    assert!(cache.get(&target).is_some());
    assert_eq!(reads.load(Ordering::Relaxed), 1);

    generation.fetch_add(1, Ordering::AcqRel);

    assert!(cache.get(&target).is_some());
    assert_eq!(
        reads.load(Ordering::Relaxed),
        2,
        "an external commit must force a fresh committed read"
    );
    let stats = cache
        .hot_key_cache_stats()
        .expect("hot-key cache is enabled");
    assert_eq!(stats.misses, 2);
    assert_eq!(stats.hits, 0);
    assert_eq!(stats.retained_entries, 1);
}

#[test]
fn concurrent_readers_share_one_committed_entry() {
    let (cache, _reads) = counting_cache(true, cache_config(false));
    let cache = Arc::new(cache);
    let target = key(7);
    let expected = target.as_bytes().into_owned();

    std::thread::scope(|scope| {
        for _ in 0..8 {
            let cache = Arc::clone(&cache);
            let target = target.clone();
            let expected = expected.clone();
            scope.spawn(move || {
                for _ in 0..64 {
                    let item = cache.get(&target).expect("committed value is present");
                    assert_eq!(item.value_bytes().as_ref(), expected.as_slice());
                }
            });
        }
    });

    let stats = cache
        .hot_key_cache_stats()
        .expect("hot-key cache is enabled");
    assert_eq!(stats.retained_entries, 1);
    assert_eq!(stats.hits + stats.misses, 8 * 64);
    assert!(stats.hits > 0);
}
