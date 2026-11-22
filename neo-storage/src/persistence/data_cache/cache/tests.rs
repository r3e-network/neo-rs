use super::*;

fn item(value: &[u8]) -> StorageItem {
    StorageItem::from_bytes(value.to_vec())
}

fn state_of(cache: &PersistenceDataCache, key: &StorageKey) -> Option<TrackState> {
    cache
        .state
        .read()
        .dictionary
        .get(key)
        .map(|entry| entry.state)
}

fn untracked_read_config() -> DataCacheConfig {
    DataCacheConfig {
        track_reads_in_write_cache: false,
        ..DataCacheConfig::default()
    }
}

#[test]
fn delete_update_delete_preserves_the_tombstone() {
    let cache = PersistenceDataCache::new(false);
    let key = StorageKey::new(1, b"k".to_vec());

    cache.add(key.clone(), item(b"v1"));
    cache.commit();
    cache.delete(&key);
    assert_eq!(state_of(&cache, &key), Some(TrackState::Deleted));

    // Re-populating a tombstone modifies pre-existing data, so the entry
    // must remain tracked as a modification rather than a fresh add.
    cache.update(key.clone(), item(b"v2"));
    assert_eq!(state_of(&cache, &key), Some(TrackState::Changed));

    // The second delete must leave a real tombstone behind instead of
    // cancelling the entry (which would resurrect the old value on commit).
    cache.delete(&key);
    assert_eq!(state_of(&cache, &key), Some(TrackState::Deleted));
    assert_eq!(
        cache.extract_raw_changes(),
        vec![(key.to_array(), None)],
        "a re-deleted key must still emit a deletion"
    );
}

#[test]
fn isolated_fork_does_not_share_the_parent_read_cache() {
    let key = StorageKey::new(1, b"shared".to_vec());
    let store_get: Arc<StoreGetFn> =
        Arc::new(|_key: &StorageKey| Some(StorageItem::from_bytes(b"parent".to_vec())));
    let parent = PersistenceDataCache::new_with_config(
        false,
        Some(store_get),
        None,
        untracked_read_config(),
    );
    assert_eq!(parent.get(&key).unwrap().to_value(), b"parent".to_vec());

    let fork = parent.fork_isolated();
    fork.update(key.clone(), item(b"fork"));

    assert_eq!(fork.get(&key).unwrap().to_value(), b"fork".to_vec());
    // A speculative fork write must never become visible to the parent.
    assert_eq!(parent.get(&key).unwrap().to_value(), b"parent".to_vec());
}

#[test]
fn external_commit_generation_drops_stale_reads() {
    let backing = Arc::new(parking_lot::Mutex::new(b"v1".to_vec()));
    let backing_get = Arc::clone(&backing);
    let store_get: Arc<StoreGetFn> = Arc::new(move |_key: &StorageKey| {
        Some(StorageItem::from_bytes(backing_get.lock().clone()))
    });
    let mut cache = PersistenceDataCache::new_with_config(
        false,
        Some(store_get),
        None,
        DataCacheConfig::default(),
    );
    let generation = Arc::new(AtomicU64::new(0));
    let generation_probe = Arc::clone(&generation);
    cache.set_generation_source(move || generation_probe.load(Ordering::Acquire));

    let key = StorageKey::new(9, b"k".to_vec());
    let pending = StorageKey::new(9, b"pending".to_vec());
    assert_eq!(cache.get(&key).unwrap().to_value(), b"v1".to_vec());
    cache.try_add(pending.clone(), item(b"pending")).unwrap();

    // A sibling writer commits a new value and advances the store version.
    *backing.lock() = b"v2".to_vec();
    generation.fetch_add(1, Ordering::AcqRel);

    assert_eq!(cache.get(&key).unwrap().to_value(), b"v2".to_vec());
    // Pending writes are owned by this cache and survive the refresh.
    assert_eq!(cache.get(&pending).unwrap().to_value(), b"pending".to_vec());
    assert_eq!(cache.pending_change_count(), 1);
}

fn find_keys(
    cache: &PersistenceDataCache,
    prefix: Option<&StorageKey>,
    direction: SeekDirection,
    skip: usize,
) -> Vec<Vec<u8>> {
    cache
        .find_with_skip(prefix, direction, skip)
        .map(|(key, _)| key.key().to_vec())
        .collect()
}

#[test]
fn find_with_skip_matches_csharp_seek_semantics() {
    let backing: Vec<(StorageKey, StorageItem)> = (0u8..5)
        .map(|i| (StorageKey::new(7, vec![0x01, i]), item(&[i])))
        .collect();
    let store_find: Arc<StoreFindFn> = Arc::new(
        move |prefix: Option<&StorageKey>, direction: SeekDirection| {
            let prefix_bytes = prefix.map(StorageKey::to_array);
            let mut entries: Vec<_> = backing
                .iter()
                .filter(|(key, _)| {
                    prefix_bytes
                        .as_ref()
                        .is_none_or(|prefix| key.as_bytes().starts_with(prefix))
                })
                .cloned()
                .collect();
            if direction == SeekDirection::Backward {
                entries.reverse();
            }
            entries
        },
    );
    let store_get: Arc<StoreGetFn> = Arc::new(|key: &StorageKey| {
        (key.key() == &[0x01u8, 0x01][..]).then(|| StorageItem::from_bytes(b"b1".to_vec()))
    });
    let cache = PersistenceDataCache::new_with_config(
        false,
        Some(store_get),
        Some(store_find),
        untracked_read_config(),
    );

    assert_eq!(
        find_keys(&cache, None, SeekDirection::Forward, 2),
        [vec![0x01, 0x02], vec![0x01, 0x03], vec![0x01, 0x04]]
    );
    assert_eq!(
        find_keys(&cache, None, SeekDirection::Forward, 5),
        Vec::<Vec<u8>>::new()
    );
    assert_eq!(
        find_keys(&cache, None, SeekDirection::Backward, 1),
        [
            vec![0x01, 0x03],
            vec![0x01, 0x02],
            vec![0x01, 0x01],
            vec![0x01, 0x00]
        ]
    );

    // Overlays participate in the skip window: a tombstone removes a
    // backing entry and a fresh add extends the tail.
    cache.add(StorageKey::new(7, vec![0x01, 0x05]), item(b"add5"));
    cache.delete(&StorageKey::new(7, vec![0x01, 0x01]));
    assert_eq!(
        find_keys(&cache, None, SeekDirection::Forward, 1),
        [
            vec![0x01, 0x02],
            vec![0x01, 0x03],
            vec![0x01, 0x04],
            vec![0x01, 0x05]
        ]
    );
}

#[test]
fn unprefixed_find_uses_serialized_byte_order() {
    // A real provider returns unprefixed scans in byte order; the overlay
    // forces the merged path where the BTreeMap would otherwise reorder
    // numerically. C# StorageKeyComparer orders 0x07 < 0x0A < 0xFB, i.e.
    // id 7, then id 10, then id -5.
    let negative = StorageKey::new(-5, vec![0x01]);
    let positive = StorageKey::new(10, vec![0x02]);
    let store_find: Arc<StoreFindFn> = Arc::new(
        move |prefix: Option<&StorageKey>, direction: SeekDirection| {
            let prefix_bytes = prefix.map(StorageKey::to_array);
            let mut entries: Vec<_> = [
                (positive.clone(), item(b"p")),
                (negative.clone(), item(b"n")),
            ]
            .into_iter()
            .filter(|(key, _)| {
                prefix_bytes
                    .as_ref()
                    .is_none_or(|prefix| key.as_bytes().starts_with(prefix))
            })
            .collect();
            if direction == SeekDirection::Backward {
                entries.reverse();
            }
            entries
        },
    );
    let store_get: Arc<StoreGetFn> = Arc::new(|_key: &StorageKey| None);
    let cache = PersistenceDataCache::new_with_config(
        false,
        Some(store_get),
        Some(store_find),
        untracked_read_config(),
    );

    cache.add(StorageKey::new(7, vec![0x03]), item(b"overlay"));

    let ids: Vec<i32> = cache
        .find(None, SeekDirection::Forward)
        .map(|(key, _)| key.id())
        .collect();
    assert_eq!(ids, [7, 10, -5], "byte order puts id 7, 10 before id -5");

    let ids: Vec<i32> = cache
        .find(None, SeekDirection::Backward)
        .map(|(key, _)| key.id())
        .collect();
    assert_eq!(ids, [-5, 10, 7]);
}

#[test]
fn cache_only_unprefixed_find_uses_serialized_byte_order() {
    let cache = PersistenceDataCache::new(false);
    cache.add(StorageKey::new(-5, vec![0x01]), item(b"n"));
    cache.add(StorageKey::new(10, vec![0x02]), item(b"p"));

    let ids: Vec<i32> = cache
        .find(None, SeekDirection::Forward)
        .map(|(key, _)| key.id())
        .collect();
    assert_eq!(ids, [10, -5]);
}
