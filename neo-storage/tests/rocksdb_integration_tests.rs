#[cfg(feature = "rocksdb")]
mod rocksdb_integration_tests {
    use neo_storage::persistence::providers::rocksdb::provider::RocksDBStoreProvider;
    use neo_storage::persistence::storage::StorageConfig;
    use neo_storage::persistence::write_batch_buffer::WriteBatchConfig;
    use neo_storage::persistence::{ReadOnlyStoreGeneric, StoreCache, StoreProvider};
    use neo_storage::{StorageItem, StorageKey};
    use tempfile::TempDir;

    #[test]
    fn rocksdb_write_batch_multiple_puts() {
        let tmp = TempDir::new().expect("tempdir");
        let db_path = tmp.path().join("batch_puts");

        let provider = RocksDBStoreProvider::new(StorageConfig {
            path: db_path.clone(),
            ..Default::default()
        })
        .with_batch_config(WriteBatchConfig::balanced());

        let store = provider.get_store("").expect("rocksdb store");

        let mut cache = StoreCache::new_from_store(store.clone(), false);

        // Add 50 items
        for i in 0..50 {
            let key = StorageKey::new(1, vec![i as u8]);
            let value = StorageItem::from_bytes(vec![i as u8 * 2]);
            cache.add(key, value);
        }

        cache.commit();

        // Verify all items
        let reader = StoreCache::new_from_store(store, false);
        for i in 0..50 {
            let key = StorageKey::new(1, vec![i as u8]);
            let result = reader.get(&key).expect("item exists");
            assert_eq!(result.value_bytes()[0], i as u8 * 2);
        }
    }

    #[test]
    fn rocksdb_write_batch_mixed_operations() {
        let tmp = TempDir::new().expect("tempdir");
        let db_path = tmp.path().join("batch_mixed");

        let provider = RocksDBStoreProvider::new(StorageConfig {
            path: db_path.clone(),
            ..Default::default()
        })
        .with_batch_config(WriteBatchConfig::balanced());

        let store = provider.get_store("").expect("rocksdb store");

        // First batch: add items
        let mut cache1 = StoreCache::new_from_store(store.clone(), false);
        for i in 0..10 {
            let key = StorageKey::new(2, vec![i]);
            let value = StorageItem::from_bytes(vec![i]);
            cache1.add(key, value);
        }
        cache1.commit();

        // Second batch: update and delete
        let mut cache2 = StoreCache::new_from_store(store.clone(), false);
        for i in 0..5 {
            let key = StorageKey::new(2, vec![i]);
            let new_value = StorageItem::from_bytes(vec![i + 100]);
            cache2.update(key, new_value);
        }
        for i in 5..10 {
            let key = StorageKey::new(2, vec![i]);
            cache2.delete(key);
        }
        cache2.commit();

        // Verify results
        let reader = StoreCache::new_from_store(store, false);
        for i in 0..5 {
            let key = StorageKey::new(2, vec![i]);
            let result = reader.get(&key).expect("updated item");
            assert_eq!(result.value_bytes()[0], i + 100);
        }
        for i in 5..10 {
            let key = StorageKey::new(2, vec![i]);
            assert!(reader.try_get(&key).is_none());
        }
    }

    #[test]
    fn rocksdb_high_throughput_config() {
        let tmp = TempDir::new().expect("tempdir");
        let db_path = tmp.path().join("high_throughput");

        let provider = RocksDBStoreProvider::new(StorageConfig {
            path: db_path.clone(),
            ..Default::default()
        })
        .with_batch_config(WriteBatchConfig::high_throughput());

        let store = provider.get_store("").expect("rocksdb store");

        let mut cache = StoreCache::new_from_store(store.clone(), false);

        // Large batch
        for i in 0..1000 {
            let key = StorageKey::new(3, vec![i as u8, (i >> 8) as u8]);
            let value = StorageItem::from_bytes(vec![i as u8]);
            cache.add(key, value);
        }

        cache.commit();

        // Spot check
        let reader = StoreCache::new_from_store(store, false);
        let key = StorageKey::new(3, vec![0, 0]);
        assert!(reader.get(&key).is_some());

        let key = StorageKey::new(3, vec![231, 3]);
        assert_eq!(reader.get(&key).expect("last written key").value(), &[231]);
    }

    #[test]
    fn rocksdb_durable_config() {
        let tmp = TempDir::new().expect("tempdir");
        let db_path = tmp.path().join("durable");

        let provider = RocksDBStoreProvider::new(StorageConfig {
            path: db_path.clone(),
            ..Default::default()
        })
        .with_batch_config(WriteBatchConfig::durable());

        let store = provider.get_store("").expect("rocksdb store");

        assert!(store.supports_durable_writes());

        let key = StorageKey::new(4, vec![0x04]).to_array();
        let value = vec![0xAA];

        let mut snapshot = store.snapshot();
        let snapshot_mut = std::sync::Arc::get_mut(&mut snapshot).expect("exclusive");
        snapshot_mut
            .put_sync(key.clone(), value.clone())
            .expect("put_sync");
        snapshot_mut.try_commit_durable().expect("durable commit");

        // Verify
        assert_eq!(store.snapshot().try_get(&key), Some(value));
    }

    #[test]
    fn rocksdb_persistence_across_reopens() {
        let tmp = TempDir::new().expect("tempdir");
        let db_path = tmp.path().join("reopen");

        let key = StorageKey::new(5, vec![0x05]).to_array();
        let value = vec![0xBB];

        // First session: write data
        {
            let provider = RocksDBStoreProvider::new(StorageConfig {
                path: db_path.clone(),
                ..Default::default()
            });
            let store = provider.get_store("").expect("rocksdb store");

            let mut snapshot = store.snapshot();
            let snapshot_mut = std::sync::Arc::get_mut(&mut snapshot).expect("exclusive");
            snapshot_mut.put(key.clone(), value.clone()).unwrap();
            snapshot_mut.try_commit().unwrap();
        }

        // Second session: read data
        {
            let provider = RocksDBStoreProvider::new(StorageConfig {
                path: db_path,
                ..Default::default()
            });
            let store = provider.get_store("").expect("rocksdb store");

            assert_eq!(store.snapshot().try_get(&key), Some(value));
        }
    }

    #[test]
    fn rocksdb_large_value_handling() {
        let tmp = TempDir::new().expect("tempdir");
        let db_path = tmp.path().join("large_values");

        let provider = RocksDBStoreProvider::new(StorageConfig {
            path: db_path,
            ..Default::default()
        });
        let store = provider.get_store("").expect("rocksdb store");

        let key = StorageKey::new(6, vec![0x06]).to_array();
        let large_value = vec![0xCC; 1024 * 1024]; // 1MB

        let mut snapshot = store.snapshot();
        let snapshot_mut = std::sync::Arc::get_mut(&mut snapshot).expect("exclusive");
        snapshot_mut.put(key.clone(), large_value.clone()).unwrap();
        snapshot_mut.try_commit().unwrap();

        let result = store.snapshot().try_get(&key);
        assert_eq!(result, Some(large_value));
    }

    #[test]
    fn rocksdb_concurrent_readers() {
        use std::sync::Arc;
        use std::thread;

        let tmp = TempDir::new().expect("tempdir");
        let db_path = tmp.path().join("concurrent_readers");

        let provider = Arc::new(RocksDBStoreProvider::new(StorageConfig {
            path: db_path,
            ..Default::default()
        }));
        let store = provider.get_store("").expect("rocksdb store");

        // Write initial data
        let mut cache = StoreCache::new_from_store(store.clone(), false);
        for i in 0..100 {
            let key = StorageKey::new(7, vec![i]);
            let value = StorageItem::from_bytes(vec![i * 2]);
            cache.add(key, value);
        }
        cache.commit();

        // Concurrent reads
        let mut handles = vec![];
        for _ in 0..10 {
            let store_clone = store.clone();
            let handle = thread::spawn(move || {
                let reader = StoreCache::new_from_store(store_clone, false);
                for i in 0..100 {
                    let key = StorageKey::new(7, vec![i]);
                    let result = reader.get(&key).expect("concurrent read");
                    assert_eq!(result.value_bytes()[0], i * 2);
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().expect("thread completion");
        }
    }

    #[test]
    fn rocksdb_stress_test_sequential_writes() {
        let tmp = TempDir::new().expect("tempdir");
        let db_path = tmp.path().join("stress_sequential");

        let provider = RocksDBStoreProvider::new(StorageConfig {
            path: db_path,
            ..Default::default()
        })
        .with_batch_config(WriteBatchConfig::high_throughput());

        let store = provider.get_store("").expect("rocksdb store");

        // Write 10,000 items
        let mut cache = StoreCache::new_from_store(store.clone(), false);
        for i in 0..10000 {
            let key = StorageKey::new(8, vec![i as u8, (i >> 8) as u8]);
            let value = StorageItem::from_bytes(vec![i as u8]);
            cache.add(key, value);
        }
        cache.commit();

        // Spot verification
        let reader = StoreCache::new_from_store(store, false);
        for i in (0..10000).step_by(100) {
            let key = StorageKey::new(8, vec![i as u8, (i >> 8) as u8]);
            assert!(reader.get(&key).is_some());
        }
    }

    #[test]
    fn rocksdb_empty_key_handling() {
        let tmp = TempDir::new().expect("tempdir");
        let db_path = tmp.path().join("empty_keys");

        let provider = RocksDBStoreProvider::new(StorageConfig {
            path: db_path,
            ..Default::default()
        });
        let store = provider.get_store("").expect("rocksdb store");

        let key = StorageKey::new(9, vec![]).to_array();
        let value = vec![0xDD];

        let mut snapshot = store.snapshot();
        let snapshot_mut = std::sync::Arc::get_mut(&mut snapshot).expect("exclusive");
        snapshot_mut.put(key.clone(), value.clone()).unwrap();
        snapshot_mut.try_commit().unwrap();

        assert_eq!(store.snapshot().try_get(&key), Some(value));
    }

    #[test]
    fn rocksdb_update_then_delete_same_transaction() {
        let tmp = TempDir::new().expect("tempdir");
        let db_path = tmp.path().join("update_delete");

        let provider = RocksDBStoreProvider::new(StorageConfig {
            path: db_path,
            ..Default::default()
        });
        let store = provider.get_store("").expect("rocksdb store");

        let key = StorageKey::new(10, vec![0x0A]);

        // Add initial value
        let mut cache1 = StoreCache::new_from_store(store.clone(), false);
        cache1.add(key.clone(), StorageItem::from_bytes(vec![0x01]));
        cache1.commit();

        // Update then delete in same transaction
        let mut cache2 = StoreCache::new_from_store(store.clone(), false);
        cache2.update(key.clone(), StorageItem::from_bytes(vec![0x02]));
        cache2.delete(key.clone());
        cache2.commit();

        // Verify deleted
        let reader = StoreCache::new_from_store(store, false);
        assert!(reader.try_get(&key).is_none());
    }
}

#[cfg(not(feature = "rocksdb"))]
mod rocksdb_integration_tests {
    #[test]
    fn rocksdb_feature_not_enabled() {
        // Guard test that keeps the suite green when the rocksdb feature is
        // not enabled
        assert!(true, "RocksDB feature not enabled, tests skipped");
    }
}
