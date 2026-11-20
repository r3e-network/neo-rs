//! Behavioural tests for speculative storage prefetching.
//!
//! Every test drives the public API only. Two seams are used deliberately:
//!
//! * `PersistenceDataCache::set_store_seek` records exactly what the prefetch asked the
//!   backing store for (prefix, cursor, direction, limit), which is the only way
//!   to prove the scan is bounded *at the source* rather than bounded after an
//!   eager `Vec` was built;
//! * `PersistenceDataCache::set_generation_source` simulates a commit landing mid-scan.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use neo_storage::persistence::data_cache::{
    DataCacheConfig, PersistenceDataCache, PrefetchConfig, PrefetchLimits, PrefetchPattern,
};
use neo_storage::persistence::providers::memory_store::MemoryStore;
use neo_storage::persistence::write_store::WriteStore;
use neo_storage::persistence::{ReadCache, SeekDirection, Store, StoreCache};
use neo_storage::{StorageItem, StorageKey};

/// Keys used by these tests: contract 1, storage prefix byte, then an index.
fn seq_key(prefix: u8, index: u8) -> StorageKey {
    StorageKey::new(1, vec![prefix, index])
}

/// Namespace prefix a scan is expected to name: contract id plus prefix byte.
fn namespace(prefix: u8) -> StorageKey {
    StorageKey::new(1, vec![prefix])
}

fn item(prefix: u8, index: u8) -> StorageItem {
    StorageItem::from_bytes(vec![prefix, index, 0xA5])
}

/// `count` sequential rows inside one namespace.
fn rows(prefix: u8, count: u8) -> Vec<(StorageKey, StorageItem)> {
    (0..count)
        .map(|index| (seq_key(prefix, index), item(prefix, index)))
        .collect()
}

/// Enabled prefetch that triggers after two observations at confidence 30.
fn active_config(count: usize, max_scan_keys: usize) -> PrefetchConfig {
    PrefetchConfig {
        enabled: true,
        count,
        confidence_threshold: 30,
        min_observations: 2,
        limits: PrefetchLimits {
            max_scan_keys,
            max_admit_keys: 8,
            max_admit_bytes: 64 * 1024,
        },
    }
}

fn cache_with(config: PrefetchConfig) -> PersistenceDataCache {
    PersistenceDataCache::new_with_config(
        false,
        None,
        None,
        DataCacheConfig {
            prefetch: config,
            ..DataCacheConfig::default()
        },
    )
}

/// One recorded call into the bounded seek seam.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SeekCall {
    prefix: Option<StorageKey>,
    start: Option<StorageKey>,
    direction: SeekDirection,
    limit: usize,
    returned: usize,
}

/// Installs a bounded, recording seek delegate over `rows`.
///
/// The delegate truncates to `limit`, which is what a real store is required to
/// do; recording the call is what lets a test prove the limit reached it.
fn install_recording_seek(
    cache: &mut PersistenceDataCache,
    rows: Vec<(StorageKey, StorageItem)>,
    calls: Arc<Mutex<Vec<SeekCall>>>,
) {
    cache.set_store_seek(move |prefix, start, direction, limit| {
        let prefix_bytes = prefix.map(StorageKey::to_array);
        let mut selected: Vec<(StorageKey, StorageItem)> = rows
            .iter()
            .filter(|(key, _)| {
                prefix_bytes
                    .as_ref()
                    .is_none_or(|bytes| key.as_bytes().starts_with(bytes))
            })
            .filter(|(key, _)| match direction {
                SeekDirection::Forward => start.is_none_or(|cursor| key > cursor),
                SeekDirection::Backward => start.is_none_or(|cursor| key < cursor),
            })
            .cloned()
            .collect();
        selected.sort_by(|left, right| left.0.cmp(&right.0));
        if direction == SeekDirection::Backward {
            selected.reverse();
        }
        selected.truncate(limit);
        calls.lock().expect("seek call log").push(SeekCall {
            prefix: prefix.cloned(),
            start: start.cloned(),
            direction,
            limit,
            returned: selected.len(),
        });
        selected
    });
}

fn recorded(calls: &Arc<Mutex<Vec<SeekCall>>>) -> Vec<SeekCall> {
    calls.lock().expect("seek call log").clone()
}

#[test]
fn forward_scan_names_the_namespace_and_excludes_the_cursor() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut cache = cache_with(active_config(3, 4));
    install_recording_seek(&mut cache, rows(0x01, 10), Arc::clone(&calls));

    for index in 0..3u8 {
        assert!(cache.get(&seq_key(0x01, index)).is_none());
    }

    let calls = recorded(&calls);
    assert_eq!(calls.len(), 1, "exactly one scan after two observations");
    let call = &calls[0];
    assert_eq!(call.direction, SeekDirection::Forward);
    assert_eq!(
        call.prefix,
        Some(namespace(0x01)),
        "the scan prefix is the storage namespace, never the whole current key"
    );
    assert_eq!(call.start, Some(seq_key(0x01, 2)), "cursor is the last key");
    assert_eq!(call.limit, 4, "the row limit reaches the backing store");
    assert_eq!(call.returned, 4);

    let metrics = cache.prefetch_metrics();
    assert_eq!(metrics.requests, 1);
    assert_eq!(metrics.keys_scanned, 4);
    assert_eq!(metrics.admitted, 3);
    assert!(metrics.bytes > 0);
}

#[test]
fn backward_scan_follows_the_detected_direction() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut cache = cache_with(active_config(3, 4));
    install_recording_seek(&mut cache, rows(0x01, 10), Arc::clone(&calls));

    for index in [5u8, 4, 3] {
        assert!(cache.get(&seq_key(0x01, index)).is_none());
    }

    let calls = recorded(&calls);
    assert_eq!(calls.len(), 1);
    let call = &calls[0];
    assert_eq!(call.direction, SeekDirection::Backward);
    assert_eq!(call.prefix, Some(namespace(0x01)));
    assert_eq!(call.start, Some(seq_key(0x01, 3)));
    assert_eq!(call.limit, 4);

    let metrics = cache.prefetch_metrics();
    assert_eq!(metrics.requests, 1);
    // Only 0, 1 and 2 exist below the cursor, so the scan stops at the edge.
    assert_eq!(metrics.keys_scanned, 3);
    assert_eq!(metrics.admitted, 3);
}

#[test]
fn the_row_ceiling_bounds_the_scan_at_the_source() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut cache = cache_with(active_config(8, 2));
    install_recording_seek(&mut cache, rows(0x01, 10), Arc::clone(&calls));

    for index in 0..3u8 {
        cache.get(&seq_key(0x01, index));
    }

    let calls = recorded(&calls);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].limit, 2, "the store is asked for two rows only");
    assert_eq!(calls[0].returned, 2);

    let metrics = cache.prefetch_metrics();
    assert_eq!(metrics.keys_scanned, 2);
    assert_eq!(metrics.admitted, 2);
}

#[test]
fn a_row_outside_the_namespace_ends_the_scan() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut cache = cache_with(active_config(4, 8));
    // A deliberately misbehaving store: it hands back a foreign namespace row
    // after two rows from the requested one.
    cache.set_store_seek(move |_prefix, _start, _direction, _limit| {
        let returned = vec![
            (seq_key(0x01, 3), item(0x01, 3)),
            (seq_key(0x01, 4), item(0x01, 4)),
            (seq_key(0x02, 0), item(0x02, 0)),
            (seq_key(0x02, 1), item(0x02, 1)),
        ];
        calls.lock().expect("seek call log").push(SeekCall {
            prefix: Some(namespace(0x01)),
            start: Some(seq_key(0x01, 2)),
            direction: SeekDirection::Forward,
            limit: 8,
            returned: returned.len(),
        });
        returned
    });

    for index in 0..3u8 {
        cache.get(&seq_key(0x01, index));
    }

    // The foreign row terminates the scan instead of being admitted.
    assert_eq!(cache.prefetch_metrics().admitted, 2);
}

#[test]
fn confidence_threshold_blocks_triggering() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let config = PrefetchConfig {
        enabled: true,
        confidence_threshold: 100,
        min_observations: 1,
        ..active_config(4, 8)
    };
    let mut cache = cache_with(config);
    install_recording_seek(&mut cache, rows(0x01, 20), Arc::clone(&calls));

    for index in 0..8u8 {
        cache.get(&seq_key(0x01, index));
    }

    assert!(
        recorded(&calls).is_empty(),
        "confidence 80 is below the configured 100"
    );
    assert_eq!(cache.prefetch_metrics().requests, 0);
    assert_eq!(cache.current_prefetch_pattern(), PrefetchPattern::None);
}

#[test]
fn enough_observations_are_required_before_the_first_scan() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let config = PrefetchConfig {
        enabled: true,
        confidence_threshold: 0,
        min_observations: 5,
        ..active_config(4, 8)
    };
    let mut cache = cache_with(config);
    install_recording_seek(&mut cache, rows(0x01, 20), Arc::clone(&calls));

    // Five reads produce four sequential observations, one short of the gate.
    for index in 0..5u8 {
        cache.get(&seq_key(0x01, index));
    }
    assert!(recorded(&calls).is_empty());
    assert_eq!(cache.prefetch_metrics().requests, 0);

    cache.get(&seq_key(0x01, 5));
    assert_eq!(recorded(&calls).len(), 1, "the fifth observation triggers");
    assert_eq!(cache.prefetch_metrics().requests, 1);
}

#[test]
fn local_writes_are_never_replaced_by_prefetched_values() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut cache = cache_with(active_config(3, 4));
    install_recording_seek(&mut cache, rows(0x01, 10), Arc::clone(&calls));

    let local = StorageItem::from_bytes(vec![0xEE, 0xEE]);
    let local_key = seq_key(0x01, 3);
    cache.add(local_key.clone(), local.clone());

    for index in 0..3u8 {
        cache.get(&seq_key(0x01, index));
    }

    let metrics = cache.prefetch_metrics();
    assert_eq!(metrics.requests, 1);
    assert_eq!(metrics.admitted, 3, "keys 4, 5 and 6 were admitted");
    assert!(
        metrics.declined >= 1,
        "the locally owned key was declined, not admitted"
    );

    // The local write still wins over the backing row for the same key.
    assert_eq!(
        cache.get(&local_key).expect("local value").to_value(),
        local.to_value()
    );
}

#[test]
fn a_commit_during_the_scan_cancels_the_batch() {
    let generation = Arc::new(AtomicU64::new(1));
    let source = Arc::clone(&generation);
    let mut cache = cache_with(active_config(3, 4));
    cache.set_generation_source(move || source.load(Ordering::Acquire));

    let bump = Arc::clone(&generation);
    cache.set_store_seek(move |_prefix, _start, _direction, limit| {
        // A commit lands while the speculative read is in flight.
        bump.fetch_add(1, Ordering::AcqRel);
        let mut selected = rows(0x01, 10);
        selected.retain(|(key, _)| *key > seq_key(0x01, 2));
        selected.truncate(limit);
        selected
    });

    for index in 0..3u8 {
        cache.get(&seq_key(0x01, index));
    }

    let metrics = cache.prefetch_metrics();
    assert_eq!(metrics.requests, 1);
    assert_eq!(metrics.canceled, 1, "the stale batch was dropped");
    assert_eq!(metrics.admitted, 0);
}

#[test]
fn generation_tagged_admission_rejects_stale_prefetches() {
    let cache: ReadCache<StorageKey, StorageItem> = ReadCache::with_defaults();
    let key = seq_key(0x01, 5);
    let stale = StorageItem::from_bytes(vec![0x01]);
    let fresh = StorageItem::from_bytes(vec![0x02]);

    cache.put(key.clone(), stale.clone(), 8);
    let source_generation = cache.generation();
    // A newer write lands after the speculative read started.
    cache.put(key.clone(), fresh.clone(), 8);

    let report = cache.put_batch_with_generation(vec![(key.clone(), stale, 8)], source_generation);
    assert_eq!(report.admitted, 0);
    assert_eq!(report.stale_skips, 1);
    assert_eq!(report.rejected, 0);
    assert_eq!(
        cache.get(&key).expect("resident value").to_value(),
        fresh.to_value(),
        "the fresher resident entry survives the stale batch"
    );

    // Same generation: the entry is resident but not fresher, so it is still
    // protected — only the metric differs.
    let report = cache.put_batch_with_generation(
        vec![(key.clone(), StorageItem::from_bytes(vec![0x03]), 8)],
        cache.generation(),
    );
    assert_eq!(report.admitted, 0);
    assert_eq!(report.rejected, 1);
    assert_eq!(report.stale_skips, 0);
    assert_eq!(
        cache.get(&key).expect("resident value").to_value(),
        fresh.to_value()
    );
}

#[test]
fn disabled_configuration_never_reads_ahead() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut cache = cache_with(PrefetchConfig::default());
    install_recording_seek(&mut cache, rows(0x01, 20), Arc::clone(&calls));

    for index in 0..10u8 {
        assert!(cache.get(&seq_key(0x01, index)).is_none());
    }

    assert!(
        recorded(&calls).is_empty(),
        "no scan may run while disabled"
    );
    assert_eq!(
        cache.prefetch_metrics(),
        neo_storage::persistence::data_cache::PrefetchMetricsSnapshot::default(),
        "every counter stays zero while disabled"
    );
    assert_eq!(cache.current_prefetch_pattern(), PrefetchPattern::None);
}

#[test]
fn a_cache_without_a_bounded_seam_never_falls_back_to_find() {
    let find_calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&find_calls);
    let find: Arc<
        dyn Fn(Option<&StorageKey>, SeekDirection) -> Vec<(StorageKey, StorageItem)> + Send + Sync,
    > = Arc::new(move |_prefix, _direction| {
        counter.fetch_add(1, Ordering::Relaxed);
        rows(0x01, 10)
    });

    let cache = PersistenceDataCache::new_with_config(
        false,
        None,
        Some(find),
        DataCacheConfig {
            prefetch: active_config(3, 4),
            ..DataCacheConfig::default()
        },
    );

    for index in 0..5u8 {
        cache.get(&seq_key(0x01, index));
    }

    assert_eq!(
        find_calls.load(Ordering::Relaxed),
        0,
        "prefetch must not materialise an unbounded range through find"
    );
    assert_eq!(cache.prefetch_metrics().requests, 0);
}

#[test]
fn store_cache_runs_a_bounded_scan_against_a_real_store() {
    let mut store = MemoryStore::new();
    for index in 0..10u8 {
        store
            .put(seq_key(0x01, index).to_array(), vec![0x01, index, 0xA5])
            .expect("seed memory store");
    }
    let store: Arc<dyn Store> = Arc::new(store);

    let cache = StoreCache::new_from_store_with_config(
        Arc::clone(&store),
        false,
        DataCacheConfig {
            prefetch: active_config(3, 8),
            ..DataCacheConfig::default()
        },
    );

    for index in 0..3u8 {
        assert!(cache.get(&seq_key(0x01, index)).is_some());
    }

    let metrics = cache.prefetch_metrics();
    assert_eq!(metrics.requests, 1, "the wired store seam ran one scan");
    assert_eq!(metrics.keys_scanned, 7, "keys 3..9 were inside the prefix");
    assert_eq!(metrics.admitted, 3, "the configured count caps admission");
    assert!(metrics.bytes > 0);

    // The next key was prefetched, so it must be served from the read cache.
    assert_eq!(
        cache
            .get(&seq_key(0x01, 3))
            .expect("prefetched key")
            .to_value(),
        item(0x01, 3).to_value()
    );
    assert!(
        cache.prefetch_metrics().useful_hits >= 1,
        "a prefetched key that was actually read counts as a useful hit"
    );

    // Prefetching must not change what a disabled cache returns.
    let plain = StoreCache::new_from_store_with_config(store, false, DataCacheConfig::default());
    for index in 0..10u8 {
        let key = seq_key(0x01, index);
        assert_eq!(
            plain.get(&key).expect("plain read").to_value(),
            cache.get(&key).expect("prefetching read").to_value(),
            "values are identical with the feature on or off"
        );
    }
    assert_eq!(
        plain.prefetch_metrics(),
        neo_storage::persistence::data_cache::PrefetchMetricsSnapshot::default()
    );
}
