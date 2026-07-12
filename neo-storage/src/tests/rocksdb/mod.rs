//! # neo-storage::tests::rocksdb
//!
//! Test module grouping RocksDB provider, store, snapshot, and write-batch
//! adapter. coverage for neo-storage.
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
use crate::persistence::RawReadOnlyStore;
use crate::persistence::StoreCache;
use crate::persistence::StoreMaintenanceBatch;
use crate::persistence::read_only_store::ReadOnlyStoreGeneric;
use crate::persistence::seek_direction::SeekDirection;
use crate::persistence::storage::StorageConfig;
use crate::persistence::store::Store;
use crate::persistence::store_snapshot::StoreSnapshot;
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
    let store = provider.get_store(&db_path).expect("rocksdb store");
    assert!(db_path.exists(), "db path should be created");

    // basic snapshot call to ensure the store is usable
    let _snapshot = store.snapshot();
}

#[test]
fn writable_open_adds_the_maintenance_column_family_to_an_existing_default_database() {
    let tmp = TempDir::new().expect("tempdir");
    let db_path = tmp.path().join("rocksdb-default-only");
    let legacy_key = b"legacy-row";
    {
        let mut options = rocksdb::Options::default();
        options.create_if_missing(true);
        let db = rocksdb::DB::open(&options, &db_path).expect("create default-only database");
        db.put(legacy_key, b"value").expect("seed default row");
    }

    let config = StorageConfig {
        path: db_path,
        ..Default::default()
    };
    let store = RocksDbStore::open(&config, WriteBatchConfig::balanced(), true, true)
        .expect("open upgraded RocksDB store");

    assert_eq!(store.try_get_bytes(legacy_key), Some(b"value".to_vec()));
    assert_eq!(
        store
            .maintenance_metadata(b"missing")
            .expect("metadata lookup"),
        None
    );
}

#[test]
fn maintenance_commit_is_atomic_and_metadata_is_not_in_the_default_column_family() {
    let tmp = TempDir::new().expect("tempdir");
    let db_path = tmp.path().join("rocksdb-maintenance");
    let config = StorageConfig {
        path: db_path.clone(),
        ..Default::default()
    };
    let data_key = StorageKey::new(-4, vec![9, 0, 0, 0, 1]).to_array();
    let metadata_key = b"ledger-pruned-through".to_vec();

    {
        let mut store = RocksDbStore::open(&config, WriteBatchConfig::balanced(), true, true)
            .expect("open RocksDB store");
        store
            .put(data_key.clone(), b"archived".to_vec())
            .expect("seed data row");

        let mut batch = StoreMaintenanceBatch::new();
        batch.delete_data(data_key.clone());
        batch.put_metadata(metadata_key.clone(), 42u32.to_be_bytes().to_vec());
        assert!(
            store
                .try_commit_durable_maintenance(&batch)
                .expect("maintenance commit")
        );

        assert_eq!(store.try_get_bytes(&data_key), None);
        assert_eq!(store.try_get_bytes(&metadata_key), None);
        assert_eq!(
            store
                .maintenance_metadata(&metadata_key)
                .expect("metadata read"),
            Some(42u32.to_be_bytes().to_vec())
        );
    }

    let reopened = RocksDbStore::open(&config, WriteBatchConfig::balanced(), true, true)
        .expect("reopen RocksDB store");
    assert_eq!(reopened.try_get_bytes(&data_key), None);
    assert_eq!(
        reopened
            .maintenance_metadata(&metadata_key)
            .expect("reopened metadata read"),
        Some(42u32.to_be_bytes().to_vec())
    );

    drop(reopened);
    let read_only_config = StorageConfig {
        path: db_path,
        read_only: true,
        ..Default::default()
    };
    let read_only = RocksDbStore::open(&read_only_config, WriteBatchConfig::balanced(), true, true)
        .expect("open RocksDB store read-only with metadata column family");
    assert_eq!(
        read_only
            .maintenance_metadata(&metadata_key)
            .expect("read-only metadata read"),
        Some(42u32.to_be_bytes().to_vec())
    );
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

    let result = provider.get_store(&file_path);
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
    let store = provider.get_store(&db_path).expect("rocksdb store");

    let key = StorageKey::new(1, vec![0x42]);
    let value1 = StorageItem::from_bytes(vec![0x01]);
    let value2 = StorageItem::from_bytes(vec![0x02]);

    let mut writer = StoreCache::new_from_store(store.clone(), false);
    writer.add(key.clone(), value1.clone());
    writer.try_commit().expect("write initial cached value");

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
    writer2.try_commit().expect("write updated cached value");

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
fn snapshot_reads_ignore_pending_writes_until_reopened_after_commit() {
    let tmp = TempDir::new().expect("tempdir");
    let db_path = tmp.path().join("rocksdb-snapshot-overlay");
    let cfg = StorageConfig {
        path: db_path.clone(),
        ..Default::default()
    };

    let provider = RocksDBStoreProvider::new(cfg);
    let store = provider.get_store(&db_path).expect("rocksdb store");

    let existing_key = StorageKey::new(7, vec![0x01]).to_array();
    let added_key = StorageKey::new(7, vec![0x02]).to_array();

    let mut writer = StoreCache::new_from_store(store.clone(), false);
    writer.add(
        StorageKey::from_bytes(&existing_key),
        StorageItem::from_bytes(vec![0xAA]),
    );
    writer.try_commit().expect("seed RocksDB snapshot rows");

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

    let entries: Vec<_> = snapshot
        .find(
            Some(&StorageKey::new(7, vec![]).to_array()),
            SeekDirection::Forward,
        )
        .collect();
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
fn raw_overlay_commit_applies_puts_and_deletes() {
    let tmp = TempDir::new().expect("tempdir");
    let cfg = StorageConfig {
        path: tmp.path().join("rocksdb-raw-overlay"),
        ..Default::default()
    };
    let mut store = RocksDbStore::open(&cfg, WriteBatchConfig::high_throughput(), true, true)
        .expect("rocksdb store");

    store
        .put(b"existing".to_vec(), b"before".to_vec())
        .expect("seed existing row");
    let overlay = [
        (b"existing".to_vec(), None),
        (b"new".to_vec(), Some(b"after".to_vec())),
    ];

    store
        .commit_raw_overlay(
            overlay
                .iter()
                .map(|(key, value)| (key.as_slice(), value.as_deref())),
        )
        .expect("commit raw overlay");

    assert_eq!(store.try_get(&b"existing".to_vec()), None);
    assert_eq!(store.try_get(&b"new".to_vec()), Some(b"after".to_vec()));
}

#[test]
fn store_cache_commits_rocksdb_store_without_snapshot_overlay() {
    let tmp = TempDir::new().expect("tempdir");
    let cfg = StorageConfig {
        path: tmp.path().join("rocksdb-store-cache-direct"),
        ..Default::default()
    };
    let store = Arc::new(
        RocksDbStore::open(&cfg, WriteBatchConfig::high_throughput(), true, true)
            .expect("rocksdb store"),
    );

    let key_keep = StorageKey::new(42, vec![0x01]);
    let key_delete = StorageKey::new(42, vec![0x02]);
    let key_add = StorageKey::new(42, vec![0x03]);

    let mut seed = StoreCache::new_from_store(store.clone(), false);
    seed.add(key_keep.clone(), StorageItem::from_bytes(vec![0x10]));
    seed.add(key_delete.clone(), StorageItem::from_bytes(vec![0x20]));
    seed.try_commit().expect("seed RocksDB store cache");

    let mut writer = StoreCache::new_from_store(store.clone(), false);
    writer.update(key_keep.clone(), StorageItem::from_bytes(vec![0x11]));
    writer.delete(key_delete.clone());
    writer.add(key_add.clone(), StorageItem::from_bytes(vec![0x30]));
    writer
        .try_commit()
        .expect("store cache direct RocksDB commit");
    writer
        .try_commit()
        .expect("second commit should be a no-op after cache is clean");

    let reader = StoreCache::new_from_store(store, false);
    assert_eq!(
        reader.get(&key_keep).map(|item| item.to_value()),
        Some(vec![0x11])
    );
    assert!(
        reader.get(&key_delete).is_none(),
        "deleted row must not survive direct RocksDB commit"
    );
    assert_eq!(
        reader.get(&key_add).map(|item| item.to_value()),
        Some(vec![0x30])
    );
}

#[test]
fn fast_sync_store_cache_raw_overlay_uses_batch_buffer_until_flush() {
    let tmp = TempDir::new().expect("tempdir");
    let cfg = StorageConfig {
        path: tmp.path().join("rocksdb-store-cache-fast-sync-buffered"),
        ..Default::default()
    };
    let store = Arc::new(
        RocksDbStore::open(&cfg, WriteBatchConfig::balanced(), true, true).expect("rocksdb store"),
    );
    store.enable_fast_sync_mode();

    for index in 0..3 {
        let key = StorageKey::new(42, vec![index]);
        let mut writer = StoreCache::new_from_store(store.clone(), false);
        writer.add(key, StorageItem::from_bytes(vec![index]));
        writer.try_commit().expect("fast-sync raw overlay commit");
    }

    let stats_before_flush = store.batch_commit_stats();
    assert_eq!(
        stats_before_flush.batches_flushed, 0,
        "fast-sync raw overlay commits should stay buffered until threshold or explicit flush"
    );
    assert_eq!(
        stats_before_flush.pending_operations, 3,
        "logical commits should accumulate in the batch buffer"
    );

    store.flush().expect("flush pending fast-sync writes");

    let stats_after_flush = store.batch_commit_stats();
    assert_eq!(stats_after_flush.batches_flushed, 1);
    assert_eq!(stats_after_flush.operations_written, 3);
    assert_eq!(stats_after_flush.pending_operations, 0);

    let reader = StoreCache::new_from_store(store, false);
    for index in 0..3 {
        let key = StorageKey::new(42, vec![index]);
        assert_eq!(
            reader.get(&key).map(|item| item.to_value()),
            Some(vec![index]),
            "flushed buffered key {index} should be durable"
        );
    }
}

#[test]
fn fast_sync_durable_store_cache_commit_bypasses_batch_buffer() {
    let tmp = TempDir::new().expect("tempdir");
    let db_path = tmp.path().join("rocksdb-store-cache-fast-sync-durable");
    let cfg = StorageConfig {
        path: db_path.clone(),
        ..Default::default()
    };
    let store = Arc::new(
        RocksDbStore::open(&cfg, WriteBatchConfig::balanced(), true, true).expect("rocksdb store"),
    );
    store.enable_fast_sync_mode();

    let key = StorageKey::new(42, vec![0xD0]);
    let mut writer = StoreCache::new_from_store(Arc::clone(&store), false);
    writer.add(key.clone(), StorageItem::from_bytes(vec![0xAB]));
    writer
        .try_commit_durable()
        .expect("canonical commit should reach durable RocksDB storage");

    assert!(
        !store.has_pending_fast_sync_writes(),
        "a canonical durability fence must not leave buffered writes"
    );
    let stats = store.batch_commit_stats();
    assert_eq!(stats.pending_operations, 0);
    assert_eq!(
        stats.batches_flushed, 0,
        "the durable overlay capability should bypass the fast-sync batch buffer"
    );

    let snapshot_reader = StoreCache::<RocksDbStore>::new_from_snapshot(store.snapshot());
    assert_eq!(
        snapshot_reader.get(&key).map(|item| item.to_value()),
        Some(vec![0xAB]),
        "a fresh backend snapshot must see the durable commit"
    );

    drop(snapshot_reader);
    drop(writer);
    drop(store);

    let reopened = Arc::new(
        RocksDbStore::open(&cfg, WriteBatchConfig::balanced(), true, true)
            .expect("reopen RocksDB store"),
    );
    let reopened_reader = StoreCache::new_from_store(reopened, false);
    assert_eq!(
        reopened_reader.get(&key).map(|item| item.to_value()),
        Some(vec![0xAB]),
        "a reopened database must retain the durable commit"
    );
}

#[test]
fn fast_sync_durable_commit_fences_previously_buffered_writes() {
    let tmp = TempDir::new().expect("tempdir");
    let db_path = tmp
        .path()
        .join("rocksdb-store-cache-fast-sync-buffered-prefix");
    let cfg = StorageConfig {
        path: db_path.clone(),
        ..Default::default()
    };
    let store = Arc::new(
        RocksDbStore::open(&cfg, WriteBatchConfig::balanced(), true, true).expect("rocksdb store"),
    );
    store.enable_fast_sync_mode();

    let buffered_key = StorageKey::new(42, vec![0xB0]);
    let durable_key = StorageKey::new(42, vec![0xD0]);
    let mut buffered_writer = StoreCache::new_from_store(Arc::clone(&store), false);
    buffered_writer.add(buffered_key.clone(), StorageItem::from_bytes(vec![0x01]));
    buffered_writer
        .try_commit()
        .expect("first overlay should enter the fast-sync buffer");
    assert_eq!(store.batch_commit_stats().pending_operations, 1);

    let mut durable_writer = StoreCache::new_from_store(Arc::clone(&store), false);
    durable_writer.add(durable_key.clone(), StorageItem::from_bytes(vec![0x02]));
    durable_writer
        .try_commit_durable()
        .expect("durable overlay must fence the buffered prefix");

    assert!(
        !store.has_pending_fast_sync_writes(),
        "the durability fence must drain every earlier fast-sync write"
    );
    assert_eq!(store.batch_commit_stats().pending_operations, 0);

    drop(buffered_writer);
    drop(durable_writer);
    drop(store);

    let reopened = Arc::new(
        RocksDbStore::open(&cfg, WriteBatchConfig::balanced(), true, true)
            .expect("reopen RocksDB store"),
    );
    let reader = StoreCache::new_from_store(reopened, false);
    assert_eq!(
        reader.get(&buffered_key).map(|item| item.to_value()),
        Some(vec![0x01]),
        "the previously buffered prefix must survive reopen"
    );
    assert_eq!(
        reader.get(&durable_key).map(|item| item.to_value()),
        Some(vec![0x02]),
        "the canonical durable overlay must survive reopen"
    );
}

#[test]
fn fast_sync_buffered_store_cache_commits_are_visible_before_flush() {
    let tmp = TempDir::new().expect("tempdir");
    let cfg = StorageConfig {
        path: tmp
            .path()
            .join("rocksdb-store-cache-fast-sync-read-your-writes"),
        ..Default::default()
    };
    let store = Arc::new(
        RocksDbStore::open(&cfg, WriteBatchConfig::balanced(), true, true).expect("rocksdb store"),
    );
    store.enable_fast_sync_mode();

    let key_keep = StorageKey::new(42, vec![0x01]);
    let key_delete = StorageKey::new(42, vec![0x02]);
    let key_add = StorageKey::new(42, vec![0x03]);

    let mut seed = StoreCache::new_from_store(store.clone(), false);
    seed.add(key_keep.clone(), StorageItem::from_bytes(vec![0x10]));
    seed.add(key_delete.clone(), StorageItem::from_bytes(vec![0x20]));
    seed.try_commit()
        .expect("seed writes should buffer in fast-sync mode");

    let mut writer = StoreCache::new_from_store(store.clone(), false);
    writer.update(key_keep.clone(), StorageItem::from_bytes(vec![0x11]));
    writer.delete(key_delete.clone());
    writer.add(key_add.clone(), StorageItem::from_bytes(vec![0x30]));
    writer
        .try_commit()
        .expect("second logical commit should remain buffered");

    let stats_before_flush = store.batch_commit_stats();
    assert_eq!(stats_before_flush.batches_flushed, 0);
    assert!(
        stats_before_flush.pending_operations > 0,
        "writes should still be buffered before explicit flush"
    );

    let reader = StoreCache::new_from_store(store.clone(), false);
    assert_eq!(
        reader.get(&key_keep).map(|item| item.to_value()),
        Some(vec![0x11]),
        "later block execution must see buffered updates before RocksDB flush"
    );
    assert!(
        reader.get(&key_delete).is_none(),
        "later block execution must see buffered deletes before RocksDB flush"
    );
    assert_eq!(
        reader.get(&key_add).map(|item| item.to_value()),
        Some(vec![0x30]),
        "later block execution must see buffered inserts before RocksDB flush"
    );

    store.flush().expect("flush pending fast-sync writes");
    let flushed_reader = StoreCache::new_from_store(store, false);
    assert_eq!(
        flushed_reader.get(&key_keep).map(|item| item.to_value()),
        Some(vec![0x11])
    );
    assert!(flushed_reader.get(&key_delete).is_none());
    assert_eq!(
        flushed_reader.get(&key_add).map(|item| item.to_value()),
        Some(vec![0x30])
    );
}

#[test]
fn fast_sync_import_shaped_store_cache_batches_buffer_until_flush() {
    let tmp = TempDir::new().expect("tempdir");
    let cfg = StorageConfig {
        path: tmp
            .path()
            .join("rocksdb-store-cache-fast-sync-import-shaped"),
        ..Default::default()
    };
    let store = Arc::new(
        RocksDbStore::open(&cfg, WriteBatchConfig::balanced(), true, true).expect("rocksdb store"),
    );
    store.enable_fast_sync_mode();

    const BATCHES: usize = 4;
    const BLOCKS_PER_BATCH: usize = 5;
    const CHANGES_PER_BLOCK: usize = 3;

    for batch in 0..BATCHES {
        let mut writer = StoreCache::new_from_store(store.clone(), false);
        for block in 0..BLOCKS_PER_BATCH {
            for change in 0..CHANGES_PER_BLOCK {
                let ordinal = (batch * BLOCKS_PER_BATCH + block) * CHANGES_PER_BLOCK + change;
                writer.add(
                    StorageKey::new(77, ordinal.to_le_bytes().to_vec()),
                    StorageItem::from_bytes(vec![batch as u8, block as u8, change as u8]),
                );
            }
        }
        writer
            .try_commit()
            .expect("fast-sync import-shaped raw overlay commit");
    }

    let stats_before_flush = store.batch_commit_stats();
    assert_eq!(
        stats_before_flush.pending_operations,
        BATCHES * BLOCKS_PER_BATCH * CHANGES_PER_BLOCK,
        "every logical import batch should accumulate in the fast-sync buffer"
    );
    assert_eq!(
        stats_before_flush.batches_flushed, 0,
        "small import-shaped batches must not force per-batch RocksDB writes"
    );

    store.flush().expect("flush pending import-shaped writes");

    let stats_after_flush = store.batch_commit_stats();
    assert_eq!(stats_after_flush.batches_flushed, 1);
    assert_eq!(
        stats_after_flush.operations_written,
        (BATCHES * BLOCKS_PER_BATCH * CHANGES_PER_BLOCK) as u64
    );
    assert_eq!(stats_after_flush.pending_operations, 0);
}

#[test]
fn fast_sync_buffered_writes_can_be_abandoned_after_failed_import() {
    let tmp = TempDir::new().expect("tempdir");
    let cfg = StorageConfig {
        path: tmp
            .path()
            .join("rocksdb-store-cache-fast-sync-abort-buffered"),
        ..Default::default()
    };
    let store = Arc::new(
        RocksDbStore::open(&cfg, WriteBatchConfig::balanced(), true, true).expect("rocksdb store"),
    );
    store.enable_fast_sync_mode();

    let key = StorageKey::new(77, b"partial-fast-sync-block".to_vec());
    let mut writer = StoreCache::new_from_store(store.clone(), false);
    writer.add(key.clone(), StorageItem::from_bytes(vec![0xAA]));
    writer
        .try_commit()
        .expect("fast-sync raw overlay commit buffers write");

    assert_eq!(store.batch_commit_stats().pending_operations, 1);

    store.discard_pending_fast_sync_writes();
    store.disable_fast_sync_mode();
    store.flush().expect("durable cleanup after failed import");

    let reader = StoreCache::new_from_store(store, false);
    assert!(
        reader.get(&key).is_none(),
        "failed fast-sync cleanup must not flush a partial unfinalized import batch"
    );
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
    let store = provider.get_store(&db_path).expect("rocksdb store");

    let key_a = StorageKey::new(-5, vec![0x1d, 0x00, 0x00, 0x00, 0x00]);
    let key_b = StorageKey::new(-5, vec![0x1d, 0x00, 0x00, 0x00, 0x05]);
    let key_other = StorageKey::new(-5, vec![0x1e, 0x00, 0x00, 0x00, 0x00]);

    let mut writer = StoreCache::new_from_store(store.clone(), false);
    writer.add(key_a.clone(), StorageItem::from_bytes(vec![0x01]));
    writer.add(key_b.clone(), StorageItem::from_bytes(vec![0x02]));
    writer.add(key_other, StorageItem::from_bytes(vec![0x03]));
    writer.try_commit().expect("seed RocksDB prefix rows");

    let prefix = StorageKey::create(-5, 0x1d);
    let forward_expected = vec![key_a.to_array(), key_b.to_array()];
    let backward_expected = vec![key_b.to_array(), key_a.to_array()];

    let store_forward_keys: Vec<Vec<u8>> = store
        .find(Some(&prefix), SeekDirection::Forward)
        .map(|(k, _)| k.to_array())
        .collect();
    assert_eq!(store_forward_keys, forward_expected);

    let store_backward_keys: Vec<Vec<u8>> = store
        .find(Some(&prefix), SeekDirection::Backward)
        .map(|(k, _)| k.to_array())
        .collect();
    assert_eq!(store_backward_keys, backward_expected);

    let snapshot_cache = StoreCache::<RocksDbStore>::new_from_snapshot(store.snapshot());
    let snapshot_forward_keys: Vec<Vec<u8>> = snapshot_cache
        .data_cache()
        .find(Some(&prefix), SeekDirection::Forward)
        .map(|(k, _)| k.to_array())
        .collect();
    assert_eq!(snapshot_forward_keys, forward_expected);

    let snapshot_backward_keys: Vec<Vec<u8>> = snapshot_cache
        .data_cache()
        .find(Some(&prefix), SeekDirection::Backward)
        .map(|(k, _)| k.to_array())
        .collect();
    assert_eq!(snapshot_backward_keys, backward_expected);
}

#[test]
fn raw_prefix_find_uses_rocksdb_prefix_bounds_in_both_directions() {
    let tmp = TempDir::new().expect("tempdir");
    let cfg = StorageConfig {
        path: tmp.path().join("rocksdb-raw-prefix-bounds"),
        ..Default::default()
    };

    let mut store =
        RocksDbStore::open(&cfg, WriteBatchConfig::balanced(), true, true).expect("rocksdb store");

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

    let snapshot = store.snapshot();
    let snapshot_forward_keys: Vec<_> = snapshot
        .find(Some(&prefix), SeekDirection::Forward)
        .map(|(key, _)| key)
        .collect();
    assert_eq!(snapshot_forward_keys, forward_expected);

    let store_keys: Vec<_> = store
        .find(Some(&prefix), SeekDirection::Backward)
        .map(|(key, _)| key)
        .collect();
    assert_eq!(store_keys, backward_expected);

    let snapshot_keys: Vec<_> = snapshot
        .find(Some(&prefix), SeekDirection::Backward)
        .map(|(key, _)| key)
        .collect();
    assert_eq!(snapshot_keys, backward_expected);
}
