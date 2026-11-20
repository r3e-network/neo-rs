#[cfg(feature = "rocksdb")]
mod rocksdb_integration_tests {
    use neo_storage::persistence::providers::rocksdb::provider::RocksDBStoreProvider;
    use neo_storage::persistence::storage::StorageConfig;
    use neo_storage::persistence::write_batch_buffer::{WriteBatchBuffer, WriteBatchConfig};
    use neo_storage::persistence::{ReadOnlyStoreGeneric, StoreCache, StoreProvider};
    use neo_storage::{StorageItem, StorageKey};
    use std::sync::Arc;
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

    /// Returns the newest `OPTIONS-*` snapshot RocksDB writes on open, which
    /// records the option set the database actually runs with.
    fn latest_options_snapshot(db_path: &std::path::Path) -> String {
        let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(db_path)
            .expect("db directory")
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("OPTIONS-"))
            })
            .collect();
        files.sort();
        let newest = files.pop().expect("rocksdb writes an OPTIONS file on open");
        std::fs::read_to_string(newest).expect("options snapshot is text")
    }

    fn option_value<'a>(snapshot: &'a str, key: &str) -> &'a str {
        let prefix = format!("{key}=");
        snapshot
            .lines()
            .find_map(|line| line.trim().strip_prefix(prefix.as_str()))
            .unwrap_or_else(|| panic!("`{key}` missing from the rocksdb options snapshot"))
    }

    fn option_flag_is_set(snapshot: &str, key: &str) -> bool {
        let value = option_value(snapshot, key).trim();
        value == "1" || value.eq_ignore_ascii_case("true")
    }

    fn option_f64(snapshot: &str, key: &str) -> f64 {
        option_value(snapshot, key)
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("`{key}` is not a number in the rocksdb options snapshot"))
    }

    /// The point-lookup preset that used to be requested through
    /// `Options::optimize_for_point_lookup` is now expressed directly. This
    /// guards that the explicit `BlockBasedOptions` keeps every setting that
    /// preset covered and that point reads over SST files stay correct.
    #[test]
    fn rocksdb_point_lookup_options_are_explicit_and_reads_stay_correct() {
        let tmp = TempDir::new().expect("tempdir");
        let db_path = tmp.path().join("point_lookup_options");

        let provider = RocksDBStoreProvider::new(StorageConfig {
            path: db_path.clone(),
            cache_size: Some(32 * 1024 * 1024),
            ..Default::default()
        });
        let store = provider.get_store("").expect("rocksdb store");

        let mut cache = StoreCache::new_from_store(store.clone(), false);
        for i in 0..128u16 {
            cache.add(
                StorageKey::new(11, i.to_be_bytes().to_vec()),
                StorageItem::from_bytes(i.to_be_bytes().to_vec()),
            );
        }
        cache.commit();
        // Drains the memtables into SST files so the reads below are served by
        // the block-based table configured in `build_db_options`.
        store.flush();

        let reader = StoreCache::new_from_store(store, false);
        for i in 0..128u16 {
            let key = StorageKey::new(11, i.to_be_bytes().to_vec());
            let item = reader.get(&key).expect("point lookup after flush");
            assert_eq!(item.value_bytes().as_ref(), i.to_be_bytes());
        }
        for i in 128..256u16 {
            let key = StorageKey::new(11, i.to_be_bytes().to_vec());
            assert!(
                reader.try_get(&key).is_none(),
                "absent key {i} must not be reported as present"
            );
        }

        let snapshot = latest_options_snapshot(&db_path);
        assert!(
            snapshot.contains("[TableOptions/BlockBasedTable \"default\"]"),
            "the snapshot must describe the block-based table the node runs"
        );
        let data_block_index = option_value(&snapshot, "data_block_index_type");
        assert!(
            data_block_index.contains("AndHash") || data_block_index == "1",
            "data-block index must keep the point-lookup hash probe, got `{data_block_index}`"
        );
        assert!(
            option_flag_is_set(&snapshot, "cache_index_and_filter_blocks"),
            "index and filter blocks must stay cached in the block cache"
        );
        assert!(
            option_flag_is_set(&snapshot, "pin_l0_filter_and_index_blocks_in_cache"),
            "L0 filter and index blocks must stay pinned"
        );
        assert!(
            option_value(&snapshot, "filter_policy")
                .to_ascii_lowercase()
                .contains("bloom"),
            "the bloom filter must stay configured"
        );
        assert!(
            (option_f64(&snapshot, "data_block_hash_table_util_ratio") - 0.75).abs() < f64::EPSILON,
            "the data-block hash ratio must stay at the point-lookup preset's 0.75"
        );
        assert!(
            option_value(&snapshot, "no_block_cache").trim() == "false",
            "the block cache must stay attached to the table reader"
        );
        assert!(
            option_flag_is_set(&snapshot, "whole_key_filtering"),
            "SST bloom filters must stay whole-key"
        );
        assert!(
            option_flag_is_set(&snapshot, "memtable_whole_key_filtering"),
            "memtable point lookups must keep whole-key filtering"
        );
        assert!(
            (option_f64(&snapshot, "memtable_prefix_bloom_size_ratio") - 0.1).abs() < f64::EPSILON,
            "removing the point-lookup preset must not shrink the memtable bloom ratio back to 0.02"
        );
    }

    #[test]
    fn write_buffer_pending_bytes_mirror_the_staged_queue() {
        let tmp = TempDir::new().expect("tempdir");
        let db = Arc::new(
            rocksdb::DB::open_default(tmp.path().join("pending_bytes")).expect("open rocksdb"),
        );
        let buffer = WriteBatchBuffer::new(
            db,
            WriteBatchConfig {
                // Keep the thresholds out of reach so the staged operation stays
                // buffered until the explicit flush below.
                max_batch_size: 1_000,
                max_delay_ms: 60_000,
                min_operations: 20,
                max_batch_bytes: 4 * 1024 * 1024,
                ..WriteBatchConfig::balanced()
            },
        );

        buffer.put(b"key", b"value").expect("stage put");

        let staged = buffer.stats_snapshot();
        assert_eq!(staged.pending_operations, 1);
        assert_eq!(staged.pending_bytes, b"key".len() + b"value".len());
        assert_eq!(staged.pending_bytes, buffer.pending_bytes());

        buffer.flush().expect("flush");

        let flushed = buffer.stats_snapshot();
        assert_eq!(flushed.pending_operations, 0);
        assert_eq!(flushed.pending_bytes, 0);
        assert_eq!(flushed.batches_flushed, 1);
        assert_eq!(flushed.operations_written, 1);
        assert_eq!(
            flushed.bytes_written as usize,
            b"key".len() + b"value".len()
        );
    }

    #[test]
    fn store_reports_acceleration_stats_from_the_active_components() {
        let tmp = TempDir::new().expect("tempdir");
        let provider = RocksDBStoreProvider::new(StorageConfig {
            path: tmp.path().join("acceleration"),
            ..Default::default()
        })
        .with_batch_config(WriteBatchConfig::balanced());
        let store = provider.get_store("").expect("rocksdb store");

        // The provider enables the read cache, so the port must report its
        // counters instead of `None`.
        let idle = store
            .acceleration_stats()
            .expect("rocksdb reports acceleration stats");
        assert!(
            idle.read_cache.is_some(),
            "read cache is enabled by default"
        );
        // The snapshot commit path calls `db.write_opt` directly, so the batch
        // buffer never stages an operation and must be reported as absent rather
        // than as an active component with fabricated zeroes.
        assert!(
            idle.write_buffer.is_none(),
            "an unfed write buffer must not be reported as active"
        );

        let mut writer = StoreCache::new_from_store(Arc::clone(&store), false);
        writer.add(
            StorageKey::new(1, vec![7u8]),
            StorageItem::from_bytes(vec![9u8; 16]),
        );
        writer.commit();

        let after = store
            .acceleration_stats()
            .expect("rocksdb reports acceleration stats");
        assert!(
            after.write_buffer.is_none(),
            "a direct commit must not invent buffered writes"
        );

        // A read through the committed generation exercises the read cache.
        let reader = StoreCache::new_from_store(Arc::clone(&store), false);
        assert!(reader.get(&StorageKey::new(1, vec![7u8])).is_some());
        let read = store
            .acceleration_stats()
            .and_then(|stats| stats.read_cache)
            .expect("read cache snapshot");
        assert!(read.hits + read.misses > 0, "reads are counted");
        assert!(
            read.current_entries
                <= neo_storage::persistence::ReadCacheConfig::default().max_entries,
            "resident entries stay inside the configured budget"
        );
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
