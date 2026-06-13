use super::*;
use crate::persistence::StoreCache;
use crate::persistence::read_only_store::ReadOnlyStoreGeneric;
use crate::persistence::seek_direction::SeekDirection;
use crate::persistence::storage::StorageConfig;
use crate::persistence::store::Store;
use crate::persistence::store_provider::StoreProvider;
use crate::persistence::write_store::WriteStore;
use crate::rocksdb::write_batch_buffer::WriteBatchConfig;
use crate::{StorageItem, StorageKey};
use std::fs;
use std::sync::Arc;
use tempfile::TempDir;

#[test]
fn opens_store_and_creates_directory() {
    let tmp = TempDir::new().expect("tempdir");
    let db_path = tmp.path().join("rocksdb");
    let cfg = StorageConfig {
        path: db_path.clone(),
        ..Default::default()
    };

    let provider = RocksDBStoreProvider::new(cfg);
    let store = provider
        .get_store(db_path.to_str().unwrap())
        .expect("rocksdb store");
    assert!(db_path.exists(), "db path should be created");

    // basic snapshot call to ensure the store is usable
    let _snapshot = store.snapshot();
}

#[test]
fn returns_error_when_path_is_file() {
    let tmp = TempDir::new().expect("tempdir");
    let file_path = tmp.path().join("not-a-dir");
    fs::write(&file_path, b"content").expect("write file");

    let cfg = StorageConfig {
        path: file_path.clone(),
        ..Default::default()
    };
    let provider = RocksDBStoreProvider::new(cfg);

    let result = provider.get_store(file_path.to_str().unwrap());
    match result {
        Ok(_) => panic!("expected failure when path is a file"),
        Err(err) => {
            assert!(
                err.to_string()
                    .to_ascii_lowercase()
                    .contains("failed to open rocksdb store"),
                "unexpected error: {err}"
            );
        }
    }
}

#[test]
fn snapshot_commit_invalidates_read_cache_for_updated_keys() {
    let tmp = TempDir::new().expect("tempdir");
    let db_path = tmp.path().join("rocksdb-cache");
    let cfg = StorageConfig {
        path: db_path.clone(),
        ..Default::default()
    };

    let provider = RocksDBStoreProvider::new(cfg);
    let store = provider
        .get_store(db_path.to_str().unwrap())
        .expect("rocksdb store");

    let key = StorageKey::new(1, vec![0x42]);
    let value1 = StorageItem::from_bytes(vec![0x01]);
    let value2 = StorageItem::from_bytes(vec![0x02]);

    let mut writer = StoreCache::new_from_store(store.clone(), false);
    writer.add(key.clone(), value1.clone());
    writer.commit();

    let reader = StoreCache::new_from_store(store.clone(), false);
    assert_eq!(
        reader
            .get(&key)
            .expect("value must exist after first write")
            .to_value(),
        value1.to_value()
    );

    let mut writer2 = StoreCache::new_from_store(store.clone(), false);
    writer2.update(key.clone(), value2.clone());
    writer2.commit();

    let reader2 = StoreCache::new_from_store(store, false);
    assert_eq!(
        reader2
            .get(&key)
            .expect("updated value must be visible")
            .to_value(),
        value2.to_value()
    );
}

#[test]
fn snapshot_reads_overlay_pending_writes_and_deletes() {
    let tmp = TempDir::new().expect("tempdir");
    let db_path = tmp.path().join("rocksdb-snapshot-overlay");
    let cfg = StorageConfig {
        path: db_path.clone(),
        ..Default::default()
    };

    let provider = RocksDBStoreProvider::new(cfg);
    let store = provider
        .get_store(db_path.to_str().unwrap())
        .expect("rocksdb store");

    let existing_key = StorageKey::new(7, vec![0x01]).to_array();
    let added_key = StorageKey::new(7, vec![0x02]).to_array();

    let mut writer = StoreCache::new_from_store(store.clone(), false);
    writer.add(
        StorageKey::from_bytes(&existing_key),
        StorageItem::from_bytes(vec![0xAA]),
    );
    writer.commit();

    let mut snapshot = store.snapshot();
    let snapshot_mut = Arc::get_mut(&mut snapshot).expect("exclusive snapshot");
    snapshot_mut.delete(existing_key.clone()).unwrap();
    snapshot_mut.put(added_key.clone(), vec![0xBB]).unwrap();

    assert_eq!(snapshot.try_get(&existing_key), None);
    assert_eq!(snapshot.try_get(&added_key), Some(vec![0xBB]));

    let entries: Vec<_> = snapshot
        .find(
            Some(&StorageKey::new(7, vec![]).to_array()),
            SeekDirection::Forward,
        )
        .collect();
    assert_eq!(entries, vec![(added_key, vec![0xBB])]);
}

#[test]
fn backward_prefix_find_returns_expected_rows_in_store_and_snapshot_views() {
    let tmp = TempDir::new().expect("tempdir");
    let db_path = tmp.path().join("rocksdb-backward-prefix");
    let cfg = StorageConfig {
        path: db_path.clone(),
        ..Default::default()
    };

    let provider = RocksDBStoreProvider::new(cfg);
    let store = provider
        .get_store(db_path.to_str().unwrap())
        .expect("rocksdb store");

    let key_a = StorageKey::new(-5, vec![0x1d, 0x00, 0x00, 0x00, 0x00]);
    let key_b = StorageKey::new(-5, vec![0x1d, 0x00, 0x00, 0x00, 0x05]);
    let key_other = StorageKey::new(-5, vec![0x1e, 0x00, 0x00, 0x00, 0x00]);

    let mut writer = StoreCache::new_from_store(store.clone(), false);
    writer.add(key_a.clone(), StorageItem::from_bytes(vec![0x01]));
    writer.add(key_b.clone(), StorageItem::from_bytes(vec![0x02]));
    writer.add(key_other, StorageItem::from_bytes(vec![0x03]));
    writer.commit();

    let prefix = StorageKey::create(-5, 0x1d);
    let expected = vec![key_b.to_array(), key_a.to_array()];

    let store_keys: Vec<Vec<u8>> = store
        .find(Some(&prefix), SeekDirection::Backward)
        .map(|(k, _)| k.to_array())
        .collect();
    assert_eq!(store_keys, expected);

    let snapshot_cache = StoreCache::new_from_snapshot(store.snapshot());
    let snapshot_keys: Vec<Vec<u8>> = snapshot_cache
        .data_cache()
        .find(Some(&prefix), SeekDirection::Backward)
        .map(|(k, _)| k.to_array())
        .collect();
    assert_eq!(snapshot_keys, expected);
}

#[test]
fn backward_raw_prefix_find_uses_rocksdb_prefix_bounds() {
    let tmp = TempDir::new().expect("tempdir");
    let cfg = StorageConfig {
        path: tmp.path().join("rocksdb-raw-prefix-bounds"),
        ..Default::default()
    };

    let mut store = RocksDbStore::open(&cfg, WriteBatchConfig::balanced(), &None, true, true)
        .expect("rocksdb store");

    for (key, value) in [
        (b"a\x00".to_vec(), vec![0x01]),
        (b"a\xff".to_vec(), vec![0x02]),
        (b"b".to_vec(), vec![0x03]),
    ] {
        store.put(key, value).expect("put raw row");
    }

    let prefix = b"a".to_vec();
    let expected = vec![b"a\xff".to_vec(), b"a\x00".to_vec()];

    let store_keys: Vec<_> = store
        .find(Some(&prefix), SeekDirection::Backward)
        .map(|(key, _)| key)
        .collect();
    assert_eq!(store_keys, expected);

    let snapshot = store.snapshot();
    let snapshot_keys: Vec<_> = snapshot
        .find(Some(&prefix), SeekDirection::Backward)
        .map(|(key, _)| key)
        .collect();
    assert_eq!(snapshot_keys, expected);
}
