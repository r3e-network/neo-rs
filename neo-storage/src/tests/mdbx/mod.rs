//! # neo-storage::tests::mdbx
//!
//! Test module grouping the production-default MDBX provider and store adapter.
//!
//! ## Boundary
//!
//! This is test/benchmark-only code for neo-storage; it may assemble fixtures
//! but must not introduce production behavior.
//!
//! ## Contents
//!
//! - Test modules and fixtures: grouped coverage for the surrounding domain.

use super::*;
use crate::persistence::{
    RawReadOnlyStore, ReadOnlyStoreGeneric, SeekDirection, Store, StoreCache, StoreSnapshot,
    WriteStore, storage::StorageConfig,
};
use crate::{StorageItem, StorageKey};
use std::fs;
use std::sync::Arc;
use tempfile::TempDir;

fn open_store(tmp: &TempDir, name: &str) -> MdbxStore {
    let provider = MdbxStoreProvider::new(StorageConfig {
        path: tmp.path().join(name),
        ..Default::default()
    });
    provider
        .get_mdbx_store(std::path::Path::new(""))
        .expect("mdbx store")
}

#[test]
fn provider_defaults_to_production_sized_tuning() {
    let provider = MdbxStoreProvider::new(StorageConfig::default());
    let tuning = provider.tuning();

    assert_eq!(tuning.map_size, 256 * 1024 * 1024 * 1024);
    assert_eq!(tuning.growth_step, 256 * 1024 * 1024);
    assert_eq!(tuning.max_readers, 4096);
}

#[test]
fn current_mdbx_wrapper_does_not_enforce_requested_max_readers() {
    let provider_source = include_str!("../../mdbx/provider.rs");
    let store_source = include_str!("../../mdbx/store.rs");

    assert!(
        provider_source.contains("does not enforce"),
        "provider docs must not claim mdbx_max_readers is enforced by the current wrapper"
    );
    assert!(
        store_source.contains("does not forward"),
        "store docs must keep the libmdbx max_readers limitation visible"
    );
    assert!(
        !store_source.contains("MDBX_opt_max_readers"),
        "current libmdbx adapter cannot claim direct max-reader enforcement"
    );
}

#[test]
fn opens_store_and_creates_environment_directory() {
    let tmp = TempDir::new().expect("tempdir");
    let db_path = tmp.path().join("nested").join("mdbx");
    let provider = MdbxStoreProvider::new(StorageConfig {
        path: db_path.clone(),
        ..Default::default()
    });

    let store = provider
        .get_store(std::path::Path::new(""))
        .expect("mdbx store");

    assert!(db_path.exists(), "MDBX environment directory should exist");
    let _snapshot = store.snapshot();
}

#[test]
fn returns_error_when_path_is_file() {
    let tmp = TempDir::new().expect("tempdir");
    let db_path = tmp.path().join("not-a-dir");
    fs::write(&db_path, b"file").expect("write file");
    let provider = MdbxStoreProvider::new(StorageConfig {
        path: db_path,
        ..Default::default()
    });

    let err = match provider.get_store(std::path::Path::new("")) {
        Ok(_) => panic!("opening a regular file as an MDBX directory should fail"),
        Err(err) => err,
    };

    let message = err.to_string().to_ascii_lowercase();
    assert!(
        message.contains("failed to open mdbx store")
            || message.contains("failed to create mdbx data directory"),
        "unexpected error: {err}"
    );
}

#[test]
fn raw_prefix_find_returns_only_matching_rows_in_both_directions() {
    let tmp = TempDir::new().expect("tempdir");
    let mut store = open_store(&tmp, "prefix");
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

    let store_backward_keys: Vec<_> = store
        .find(Some(&prefix), SeekDirection::Backward)
        .map(|(key, _)| key)
        .collect();
    assert_eq!(store_backward_keys, backward_expected);

    let snapshot = store.snapshot();
    let snapshot_forward_keys: Vec<_> = snapshot
        .find(Some(&prefix), SeekDirection::Forward)
        .map(|(key, _)| key)
        .collect();
    assert_eq!(snapshot_forward_keys, forward_expected);

    let snapshot_backward_keys: Vec<_> = snapshot
        .find(Some(&prefix), SeekDirection::Backward)
        .map(|(key, _)| key)
        .collect();
    assert_eq!(snapshot_backward_keys, backward_expected);
}

#[test]
fn snapshot_reads_ignore_pending_writes_until_reopened_after_commit() {
    let tmp = TempDir::new().expect("tempdir");
    let mut store = open_store(&tmp, "snapshot");
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
fn store_cache_commits_mdbx_store_without_snapshot_overlay() {
    let tmp = TempDir::new().expect("tempdir");
    let store = Arc::new(open_store(&tmp, "store-cache"));

    let key_keep = StorageKey::new(42, vec![0x01]);
    let key_delete = StorageKey::new(42, vec![0x02]);
    let key_add = StorageKey::new(42, vec![0x03]);

    let mut seed = StoreCache::new_from_store(store.clone(), false);
    seed.add(key_keep.clone(), StorageItem::from_bytes(vec![0x10]));
    seed.add(key_delete.clone(), StorageItem::from_bytes(vec![0x20]));
    seed.commit();

    let mut writer = StoreCache::new_from_store(store.clone(), false);
    writer.update(key_keep.clone(), StorageItem::from_bytes(vec![0x11]));
    writer.delete(key_delete.clone());
    writer.add(key_add.clone(), StorageItem::from_bytes(vec![0x30]));
    writer.try_commit().expect("store cache direct MDBX commit");
    writer
        .try_commit()
        .expect("second commit should be a no-op after cache is clean");

    let reader = StoreCache::new_from_store(store, false);
    assert_eq!(
        reader.get(&key_keep).map(|item| item.to_value()),
        Some(vec![0x11])
    );
    assert!(reader.get(&key_delete).is_none());
    assert_eq!(
        reader.get(&key_add).map(|item| item.to_value()),
        Some(vec![0x30])
    );
}

#[test]
fn snapshot_open_does_not_materialize_entire_mdbx_namespace() {
    let source = include_str!("../../mdbx/snapshot.rs");

    assert!(
        !source.contains("snapshot_entries"),
        "MDBX snapshots must use an MVCC read transaction instead of full keyspace materialization"
    );
    assert!(
        !source.contains("immutable_data"),
        "MDBX snapshots must not clone the full backend into memory"
    );
}

#[test]
fn backward_prefix_find_uses_reverse_cursor_without_forward_materialization() {
    let store_source = include_str!("../../mdbx/store.rs");
    let snapshot_source = include_str!("../../mdbx/snapshot.rs");

    for (name, source) in [("store", store_source), ("snapshot", snapshot_source)] {
        assert!(
            !source.contains("entries.reverse()"),
            "MDBX {name} backward scans must traverse the cursor backward instead of collecting forward rows"
        );
    }
}

#[test]
fn typed_prefix_find_matches_storage_key_rows() {
    let tmp = TempDir::new().expect("tempdir");
    let store = Arc::new(open_store(&tmp, "typed-prefix"));
    let key_a = StorageKey::new(-5, vec![0x1d, 0x00]);
    let key_b = StorageKey::new(-5, vec![0x1d, 0xff]);
    let key_other = StorageKey::new(-5, vec![0x1e, 0x00]);

    let mut writer = StoreCache::new_from_store(store.clone(), false);
    writer.add(key_a.clone(), StorageItem::from_bytes(vec![0x01]));
    writer.add(key_b.clone(), StorageItem::from_bytes(vec![0x02]));
    writer.add(key_other, StorageItem::from_bytes(vec![0x03]));
    writer.try_commit().expect("commit typed rows");

    let prefix = StorageKey::create(-5, 0x1d);
    let cache = StoreCache::new_from_store(store, false);
    let keys: Vec<_> = cache
        .find(Some(&prefix), SeekDirection::Backward)
        .map(|(key, _)| key.to_array())
        .collect();

    assert_eq!(keys, vec![key_b.to_array(), key_a.to_array()]);
}

#[test]
fn data_persists_after_reopen() {
    let tmp = TempDir::new().expect("tempdir");
    let db_path = tmp.path().join("persist");
    let provider = MdbxStoreProvider::new(StorageConfig {
        path: db_path,
        ..Default::default()
    });
    let key = b"persisted".to_vec();

    {
        let mut store = provider
            .get_mdbx_store(std::path::Path::new(""))
            .expect("open mdbx store");
        store
            .put(key.clone(), b"value".to_vec())
            .expect("write persisted row");
    }

    let reopened = provider
        .get_mdbx_store(std::path::Path::new(""))
        .expect("reopen mdbx store");
    assert_eq!(reopened.try_get(&key), Some(b"value".to_vec()));
}
