use super::*;
use crate::types::storage_item::CacheProvider;
use std::any::Any;

#[derive(Clone, Debug)]
struct BytesCache(Vec<u8>);

impl CacheProvider for BytesCache {
    fn to_bytes(&self) -> Vec<u8> {
        self.0.clone()
    }

    fn clone_box(&self) -> Box<dyn CacheProvider> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn cache_item(bytes: Vec<u8>) -> StorageItem {
    let mut item = StorageItem::new();
    item.set_cache(Box::new(BytesCache(bytes)));
    item
}

#[test]
fn find_without_prefix_uses_csharp_storage_byte_order() {
    let cache = DataCache::new(false);
    let negative_id = StorageKey::new(-5, vec![0x01]);
    let positive_id = StorageKey::new(10, vec![0x01]);
    let zero_id = StorageKey::new(0, vec![0xFF]);

    for key in [&negative_id, &positive_id, &zero_id] {
        cache.add(
            key.clone(),
            StorageItem::from_bytes(vec![key.to_array()[0]]),
        );
    }

    let found: Vec<_> = cache
        .find(None, SeekDirection::Forward)
        .map(|(key, _)| key.to_array())
        .collect();

    let mut expected = vec![
        negative_id.to_array(),
        positive_id.to_array(),
        zero_id.to_array(),
    ];
    expected.sort();

    assert_eq!(
        found, expected,
        "C# v3.10 DataCache.Seek sorts StorageKey.ToArray() with ByteArrayComparer, not numeric contract ids"
    );
}

#[test]
fn try_add_rejects_live_cached_entry_like_csharp() {
    let cache = DataCache::new(false);
    let key = StorageKey::new(7, vec![0x01]);

    cache
        .try_add(key.clone(), StorageItem::from_bytes(vec![0xAA]))
        .expect("initial add");
    let err = cache
        .try_add(key.clone(), StorageItem::from_bytes(vec![0xBB]))
        .expect_err("C# DataCache.Add throws when the cached entry is already live");

    assert_eq!(err, DataCacheError::InvalidState(TrackState::Added));
    assert_eq!(
        cache.get(&key).expect("cached item").value_bytes().as_ref(),
        &[0xAA],
        "failed Add must not overwrite the existing cached item"
    );
}

#[test]
fn extract_raw_changes_materializes_cache_backed_storage_item_value() {
    let cache = DataCache::new(false);
    let key = StorageKey::new(7, vec![0x03]);
    cache
        .try_add(key.clone(), cache_item(vec![0xCA, 0xFE]))
        .expect("add cache-backed item");

    assert_eq!(
        cache.extract_raw_changes(),
        vec![(key.to_array(), Some(vec![0xCA, 0xFE]))],
        "C# StorageItem.Value materializes cache-backed values before persistence"
    );
}

#[test]
fn try_add_after_deleted_cached_entry_becomes_changed_like_csharp() {
    let key = StorageKey::new(7, vec![0x02]);
    let stored_key = key.clone();
    let store_get: Arc<StoreGetFn> = Arc::new(move |lookup: &StorageKey| {
        if lookup == &stored_key {
            Some(StorageItem::from_bytes(vec![0xAA]))
        } else {
            None
        }
    });
    let cache = DataCache::new_with_store(false, Some(store_get), None);

    cache.delete(&key);
    cache
        .try_add(key.clone(), StorageItem::from_bytes(vec![0xBB]))
        .expect("C# DataCache.Add transitions Deleted -> Changed");

    let tracked = cache.tracked_items();
    let (_, trackable) = tracked
        .iter()
        .find(|(tracked_key, _)| tracked_key == &key)
        .expect("tracked key");
    assert_eq!(trackable.state, TrackState::Changed);
    assert_eq!(trackable.item.value_bytes().as_ref(), &[0xBB]);
}

#[test]
fn update_after_deleted_cached_entry_becomes_changed_not_added() {
    // Regression for the MainNet block-166739 GAS over-credit: a store-backed
    // key that is deleted (balance burned to exactly zero) and then re-created
    // via `update` (re-credited) must transition Deleted -> Changed, mirroring
    // C# `DataCache.GetAndChange`. The prior Rust behaviour used Added, which
    // makes the later commit go through `add()` and get swallowed when the
    // parent has the key read-cached as `None`.
    let key = StorageKey::new(7, vec![0x05]);
    let stored_key = key.clone();
    let store_get: Arc<StoreGetFn> = Arc::new(move |lookup: &StorageKey| {
        if lookup == &stored_key {
            Some(StorageItem::from_bytes(vec![0xAA]))
        } else {
            None
        }
    });
    let cache = DataCache::new_with_store(false, Some(store_get), None);

    cache.delete(&key);
    cache.update(key.clone(), StorageItem::from_bytes(vec![0xBB]));

    let tracked = cache.tracked_items();
    let (_, trackable) = tracked
        .iter()
        .find(|(tracked_key, _)| tracked_key == &key)
        .expect("tracked key");
    assert_eq!(trackable.state, TrackState::Changed);
    assert_eq!(trackable.item.value_bytes().as_ref(), &[0xBB]);
}

#[test]
fn delete_then_recreate_persists_through_layered_commit() {
    // End-to-end reproduction of the consensus bug across the production cache
    // hierarchy store -> block snapshot -> block cache (as in
    // `persist_block_natives`): a key present in the store, read by an upper
    // layer (so it is cached as `None`), then deleted and re-created via
    // `update` in the child, must persist the new value down to the store —
    // not the stale pre-deletion value.
    let key = StorageKey::new(-6, vec![0x14, 0x01]);

    let store = DataCache::new(false);
    store.add(key.clone(), StorageItem::from_bytes(vec![0xAA]));

    let snapshot = store.clone_cache();
    let block_cache = snapshot.clone_cache();

    // Read through both overlays — populates the read cache as `None`.
    assert_eq!(
        block_cache.get(&key).map(|i| i.value_bytes().to_vec()),
        Some(vec![0xAA])
    );

    block_cache.delete(&key);
    block_cache.update(key.clone(), StorageItem::from_bytes(vec![0xBB]));

    block_cache.commit();
    snapshot.commit();

    assert_eq!(
        store.get(&key).map(|i| i.value_bytes().to_vec()),
        Some(vec![0xBB]),
        "delete-then-recreate must persist the new value, not the stale original"
    );
}
