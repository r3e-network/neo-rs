use super::*;
use crate::persistence::store_cache::StoreCache;
use crate::persistence::store_snapshot::StoreSnapshot;

#[test]
fn raw_prefix_find_returns_only_matching_rows_in_both_directions() {
    let mut store = MemoryStore::new();
    for (key, value) in [
        (b"a\x00".to_vec(), vec![0x01]),
        (b"a\xff".to_vec(), vec![0x02]),
        (b"b".to_vec(), vec![0x03]),
    ] {
        store.put(key, value).expect("put raw row");
    }

    let prefix = b"a".to_vec();
    let forward_expected = vec![b"a\x00".to_vec(), b"a\xff".to_vec()];
    let backward_expected = vec![b"a\xff".to_vec(), b"a\x00".to_vec()];

    let store_forward_keys: Vec<_> = store
        .find(Some(&prefix), SeekDirection::Forward)
        .map(|(key, _)| key)
        .collect();
    assert_eq!(store_forward_keys, forward_expected);

    let store_keys: Vec<_> = store
        .find(Some(&prefix), SeekDirection::Backward)
        .map(|(key, _)| key)
        .collect();
    assert_eq!(store_keys, backward_expected);

    let snapshot = store.snapshot();
    let snapshot_forward_keys: Vec<_> = snapshot
        .find(Some(&prefix), SeekDirection::Forward)
        .map(|(key, _)| key)
        .collect();
    assert_eq!(snapshot_forward_keys, forward_expected);

    let snapshot_keys: Vec<_> = snapshot
        .find(Some(&prefix), SeekDirection::Backward)
        .map(|(key, _)| key)
        .collect();
    assert_eq!(snapshot_keys, backward_expected);
}

#[test]
fn snapshot_reads_ignore_pending_writes_until_reopened_after_commit() {
    let mut store = MemoryStore::new();
    let existing_key = b"k1".to_vec();
    let added_key = b"k2".to_vec();

    store
        .put(existing_key.clone(), vec![0xAA])
        .expect("put existing row");

    let mut snapshot = store.snapshot();
    {
        let snapshot_mut = Arc::get_mut(&mut snapshot).expect("exclusive snapshot");
        snapshot_mut.delete(existing_key.clone()).unwrap();
        snapshot_mut.put(added_key.clone(), vec![0xBB]).unwrap();
    }

    assert_eq!(snapshot.try_get(&existing_key), Some(vec![0xAA]));
    assert_eq!(
        snapshot.try_get_bytes(existing_key.as_slice()),
        Some(vec![0xAA])
    );
    assert_eq!(snapshot.try_get(&added_key), None);
    assert_eq!(snapshot.try_get_bytes(added_key.as_slice()), None);
    let entries: Vec<_> = snapshot.find(None, SeekDirection::Forward).collect();
    assert_eq!(entries, vec![(existing_key.clone(), vec![0xAA])]);

    Arc::get_mut(&mut snapshot)
        .expect("exclusive snapshot")
        .try_commit()
        .expect("snapshot commit");

    assert_eq!(snapshot.try_get(&existing_key), Some(vec![0xAA]));
    assert_eq!(
        snapshot.try_get_bytes(existing_key.as_slice()),
        Some(vec![0xAA])
    );
    assert_eq!(snapshot.try_get(&added_key), None);
    assert_eq!(snapshot.try_get_bytes(added_key.as_slice()), None);

    let reopened = store.snapshot();
    assert_eq!(reopened.try_get(&existing_key), None);
    assert_eq!(reopened.try_get_bytes(existing_key.as_slice()), None);
    assert_eq!(reopened.try_get(&added_key), Some(vec![0xBB]));
    assert_eq!(
        reopened.try_get_bytes(added_key.as_slice()),
        Some(vec![0xBB])
    );
}

#[test]
fn snapshot_commit_applies_batch_without_runtime_parent_downcast() {
    let store = MemoryStore::new();
    let key = b"typed-parent".to_vec();
    let mut snapshot = store.snapshot();

    Arc::get_mut(&mut snapshot)
        .expect("exclusive snapshot")
        .put(key.clone(), vec![0xCA])
        .expect("stage put");
    Arc::get_mut(&mut snapshot)
        .expect("exclusive snapshot")
        .try_commit()
        .expect("commit snapshot");

    assert_eq!(store.try_get(&key), Some(vec![0xCA]));
}

#[test]
fn snapshot_backed_store_cache_backward_find_matches_prefix_rows() {
    let mut store = MemoryStore::new();
    let key_a = StorageKey::new(-5, vec![0x1d, 0x00]);
    let key_b = StorageKey::new(-5, vec![0x1d, 0xff]);
    let key_other = StorageKey::new(-5, vec![0x1e, 0x00]);

    for (key, value) in [
        (key_a.to_array(), vec![0x01]),
        (key_b.to_array(), vec![0x02]),
        (key_other.to_array(), vec![0x03]),
    ] {
        store.put(key, value).expect("put storage row");
    }

    let prefix = StorageKey::create(-5, 0x1d);
    let cache = StoreCache::<MemoryStore>::new_from_snapshot(store.snapshot());
    let keys: Vec<_> = cache
        .data_cache()
        .find(Some(&prefix), SeekDirection::Backward)
        .map(|(key, _)| key.to_array())
        .collect();

    assert_eq!(keys, vec![key_b.to_array(), key_a.to_array()]);
}
