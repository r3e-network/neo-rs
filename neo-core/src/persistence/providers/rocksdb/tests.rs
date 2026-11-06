//! Compatibility regressions only; backend behavior tests live in neo-storage.

use super::*;
use crate::error::CoreResult;
use crate::persistence::{
    read_only_store::ReadOnlyStoreGeneric, storage::StorageConfig, store::Store,
    store_provider::StoreProvider, write_batch_buffer::WriteBatchConfig, write_store::WriteStore,
};
use neo_storage::{StorageError, StorageItem, StorageKey, StorageResult};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tempfile::TempDir;

#[test]
fn facade_provider_downcast_and_callback_identity_remain_core_owned() {
    let tmp = TempDir::new().unwrap();
    let provider = RocksDBStoreProvider::new(StorageConfig {
        path: tmp.path().join("provider-downcast-compat"),
        ..Default::default()
    });
    assert_eq!(provider.name(), "RocksDBStore");
    let store = provider.get_store("").unwrap();
    assert!(store.as_any().is::<RocksDbStore>());
    assert!(
        !store
            .as_any()
            .is::<neo_storage::persistence::providers::rocksdb::RocksDbStore>()
    );

    let callbacks = Arc::new(AtomicUsize::new(0));
    let callbacks_inner = Arc::clone(&callbacks);
    store.on_new_snapshot(Box::new(move |callback_store, snapshot| {
        assert!(callback_store.as_any().is::<RocksDbStore>());
        assert!(snapshot.store().as_any().is::<RocksDbStore>());
        callbacks_inner.fetch_add(1, Ordering::Relaxed);
    }));
    let mut snapshot = store.snapshot();
    assert_eq!(callbacks.load(Ordering::Relaxed), 1);
    // No intermediate snapshot wrapper: the callback sees this same exclusive snapshot.
    assert!(Arc::get_mut(&mut snapshot).is_some());
    let first = snapshot.store();
    let second = snapshot.store();
    assert!(Arc::ptr_eq(&first, &second));
}

#[test]
fn facade_flush_retains_core_result_without_batching_direct_writes() {
    let tmp = TempDir::new().unwrap();
    let provider = RocksDBStoreProvider::new(StorageConfig {
        path: tmp.path().join("store-flush-compat"),
        ..Default::default()
    });
    let store = provider.get_store("").unwrap();
    let facade = store.as_any().downcast_ref::<RocksDbStore>().unwrap();
    let mut cloned = facade.clone();
    cloned.put(b"key".to_vec(), b"value".to_vec()).unwrap();
    assert_eq!(facade.try_get(&b"key".to_vec()), Some(b"value".to_vec()));
    assert_eq!(facade.batch_commit_stats().operations_written, 0);
    let result: CoreResult<()> = facade.flush_batch_writes();
    result.unwrap();
    let result: CoreResult<()> = cloned.flush_batch_writes();
    result.unwrap();
    assert_eq!(facade.batch_commit_stats().operations_written, 0);
    cloned.enable_read_cache(Default::default());
    assert!(cloned.read_cache_stats().is_some());
    cloned.disable_read_cache();
    assert!(cloned.read_cache_stats().is_none());
}

#[test]
fn facade_public_errors_preserve_storage_io_and_commit_failed_messages() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("not-a-directory");
    std::fs::write(&file, b"file").unwrap();
    let error = RocksDBStoreProvider::new(StorageConfig {
        path: file.clone(),
        ..Default::default()
    })
    .get_store("")
    .err()
    .expect("opening a file must fail");
    match &error {
        StorageError::Io { message } => assert!(message.starts_with(&format!(
            "failed to open RocksDB store at {}: ",
            file.display()
        ))),
        other => panic!("unexpected provider error: {other}"),
    }

    let path = tmp.path().join("read-only-compat");
    drop(
        RocksDBStoreProvider::new(StorageConfig {
            path: path.clone(),
            ..Default::default()
        })
        .get_store("")
        .unwrap(),
    );
    let store = RocksDBStoreProvider::new(StorageConfig {
        path,
        read_only: true,
        ..Default::default()
    })
    .get_store("")
    .unwrap();
    let mut facade = store
        .as_any()
        .downcast_ref::<RocksDbStore>()
        .unwrap()
        .clone();
    let result: StorageResult<()> = facade.put(b"key".to_vec(), b"value".to_vec());
    match result.unwrap_err() {
        StorageError::Io { message } => assert!(message.starts_with("RocksDB put failed: ")),
        other => panic!("unexpected direct-write error: {other}"),
    }
    let mut snapshot = store.snapshot();
    let snapshot = Arc::get_mut(&mut snapshot).unwrap();
    snapshot.put(b"key".to_vec(), b"value".to_vec()).unwrap();
    let result: StorageResult<()> = snapshot.try_commit_durable();
    match result.unwrap_err() {
        StorageError::CommitFailed(message) => {
            assert!(message.starts_with("RocksDB write failed: "))
        }
        other => panic!("unexpected snapshot error: {other}"),
    }
}

#[test]
fn facade_clone_shared_snapshot_preserves_history_and_durable_commit() {
    let tmp = TempDir::new().unwrap();
    let provider = RocksDBStoreProvider::new(StorageConfig {
        path: tmp.path().join("shared-snapshot-compat"),
        ..Default::default()
    })
    .with_batch_config(WriteBatchConfig::high_throughput());
    let store = provider.get_store("").unwrap();
    let facade = store.as_any().downcast_ref::<RocksDbStore>().unwrap();
    let mut cloned = facade.clone();
    let key = StorageKey::new(7, vec![0x42]);
    let raw_key = key.to_array();
    cloned.put_sync(raw_key.clone(), vec![0x01]).unwrap();
    let historical = facade.snapshot();
    cloned.put(raw_key.clone(), vec![0x02]).unwrap();
    assert_eq!(facade.try_get(&key).unwrap().to_value(), vec![0x02]);
    assert_eq!(historical.try_get(&raw_key), Some(vec![0x01]));

    let mut snapshot = cloned.snapshot();
    let exposed = snapshot.store();
    assert!(exposed.as_any().is::<RocksDbStore>());
    assert!(Arc::ptr_eq(&exposed, &snapshot.store()));
    let writer = Arc::get_mut(&mut snapshot).unwrap();
    writer
        .put(
            raw_key.clone(),
            StorageItem::from_bytes(vec![0x03]).to_value(),
        )
        .unwrap();
    let result: StorageResult<()> = writer.try_commit_durable();
    result.unwrap();
    assert_eq!(facade.try_get(&key).unwrap().to_value(), vec![0x03]);
    assert_eq!(exposed.try_get(&key).unwrap().to_value(), vec![0x03]);
    assert_eq!(historical.try_get(&raw_key), Some(vec![0x01]));
    assert!(facade.supports_durable_writes());
    drop(exposed);
    drop(snapshot);
    drop(cloned);
    drop(store);
    assert_eq!(historical.try_get(&raw_key), Some(vec![0x01]));
    // The snapshot's store retains the same raw DB handle after the original facade drops.
    assert_eq!(
        historical.store().try_get(&key).unwrap().to_value(),
        vec![0x03]
    );
}
