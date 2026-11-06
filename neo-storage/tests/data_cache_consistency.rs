use neo_storage::persistence::DataCache;
use neo_storage::persistence::data_cache::DataCacheConfig;
use neo_storage::{StorageItem, StorageKey};
use std::sync::Arc;

fn cached_parent(key: &StorageKey, initial: &[u8]) -> DataCache {
    let initial = StorageItem::from_bytes(initial.to_vec());
    let store_get = {
        let key = key.clone();
        let initial = initial.clone();
        Arc::new(move |requested: &StorageKey| (requested == &key).then(|| initial.clone()))
    };
    DataCache::new_with_config(
        false,
        Some(store_get),
        None,
        DataCacheConfig {
            enable_read_cache: true,
            enable_prefetching: true,
            ..Default::default()
        },
    )
}

#[test]
fn sibling_overlay_observes_committed_value_after_earlier_cached_read() {
    let key = StorageKey::new(438, vec![0x46, 0x0e]);
    let parent = cached_parent(&key, &[0xf1, 0x43, 0x7c]);

    let stale_reader = parent.clone_cache();
    assert_eq!(
        stale_reader.get(&key).unwrap().value_bytes().as_ref(),
        &[0xf1, 0x43, 0x7c]
    );

    let writer = parent.clone_cache();
    writer.update(key.clone(), StorageItem::from_bytes(vec![0xf0, 0x43, 0x7c]));
    writer.commit();

    assert_eq!(
        parent.get(&key).unwrap().value_bytes().as_ref(),
        &[0xf0, 0x43, 0x7c]
    );
    assert_eq!(
        stale_reader.get(&key).unwrap().value_bytes().as_ref(),
        &[0xf0, 0x43, 0x7c]
    );
}

#[test]
fn prefetch_does_not_overwrite_a_newer_cached_value() {
    let cache = neo_storage::persistence::read_cache::ReadCache::with_defaults();
    let key = StorageKey::new(438, vec![0x46, 0x0e]);
    cache.put(
        key.clone(),
        StorageItem::from_bytes(vec![0xf0, 0x43, 0x7c]),
        3,
    );

    cache.put_batch(vec![(
        key.clone(),
        StorageItem::from_bytes(vec![0xf1, 0x43, 0x7c]),
        3,
    )]);

    assert_eq!(
        cache.get(&key).unwrap().value_bytes().as_ref(),
        &[0xf0, 0x43, 0x7c]
    );
}
