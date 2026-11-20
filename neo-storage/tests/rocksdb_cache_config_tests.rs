//! Independent row-cache configuration and engine cache statistics.
//!
//! Every case opens a real RocksDB database: the knob only matters once it
//! reaches the engine, and the statistics only mean anything if they describe a
//! database that actually ran.

#[cfg(feature = "rocksdb")]
mod rocksdb_cache_config_tests {
    use neo_storage::persistence::providers::rocksdb::provider::RocksDBStoreProvider;
    use neo_storage::persistence::storage::StorageConfig;
    use neo_storage::persistence::{SeekDirection, Store, StoreCache, StoreProvider};
    use neo_storage::{StorageItem, StorageKey};
    use std::sync::Arc;
    use tempfile::TempDir;

    const MIB: usize = 1024 * 1024;

    fn provider_with(
        row_cache_size_bytes: Option<usize>,
        block_cache: usize,
    ) -> (TempDir, RocksDBStoreProvider) {
        let tmp = TempDir::new().expect("tempdir");
        let provider = RocksDBStoreProvider::new(StorageConfig {
            path: tmp.path().join("cache-config"),
            cache_size: Some(block_cache),
            row_cache_size_bytes,
            ..Default::default()
        });
        (tmp, provider)
    }

    fn write_keys(store: &Arc<dyn Store>, prefix: i32, count: u32, value_len: usize) {
        let mut cache = StoreCache::new_from_store(Arc::clone(store), false);
        for i in 0..count {
            let key = StorageKey::new(prefix, i.to_be_bytes().to_vec());
            cache.add(
                key,
                StorageItem::from_bytes(vec![(i % 251) as u8; value_len]),
            );
        }
        cache.commit();
    }

    /// An unset row cache keeps the historical derivation unchanged.
    #[test]
    fn default_config_derives_the_row_cache_from_the_block_cache() {
        let (_tmp, provider) = provider_with(None, 512 * MIB);
        let store = provider.get_rocksdb_store("").expect("rocksdb store");

        let stats = store.cache_stats();
        assert_eq!(stats.block_cache.capacity, 512 * MIB);
        let row_cache = stats
            .row_cache
            .expect("the default configuration must keep a row cache");
        assert_eq!(
            row_cache.capacity,
            (512 * MIB) / 4,
            "the derived row cache must stay a quarter of the block cache"
        );
    }

    /// The derived default still clamps, exactly as it did before the knob.
    #[test]
    fn default_config_keeps_the_derived_clamp_at_both_ends() {
        let (_small_tmp, small_provider) = provider_with(None, 32 * MIB);
        let small = small_provider
            .get_rocksdb_store("")
            .expect("rocksdb store")
            .cache_stats();
        assert_eq!(
            small.row_cache.expect("row cache").capacity,
            64 * MIB,
            "a small block cache must still yield the 64 MiB floor"
        );

        let (_large_tmp, large_provider) = provider_with(None, 8 * 1024 * MIB);
        let large = large_provider
            .get_rocksdb_store("")
            .expect("rocksdb store")
            .cache_stats();
        assert_eq!(
            large.row_cache.expect("row cache").capacity,
            512 * MIB,
            "a large block cache must still yield the 512 MiB ceiling"
        );
    }

    /// `Some(0)` removes the row cache instead of allocating an empty one.
    #[test]
    fn zero_row_cache_size_disables_the_row_cache() {
        let (_tmp, provider) = provider_with(Some(0), 512 * MIB);
        let store = provider.get_rocksdb_store("").expect("rocksdb store");

        let stats = store.cache_stats();
        assert!(
            stats.row_cache.is_none(),
            "a zero-sized row cache must be absent, not empty"
        );
        assert_eq!(stats.block_cache.capacity, 512 * MIB);
    }

    /// An explicit size is honoured verbatim, including outside the clamp.
    #[test]
    fn explicit_row_cache_size_is_used_verbatim() {
        let (_tmp, provider) = provider_with(Some(256 * MIB), 1024 * MIB);
        let store = provider.get_rocksdb_store("").expect("rocksdb store");

        let stats = store.cache_stats();
        assert_eq!(stats.row_cache.expect("row cache").capacity, 256 * MIB);
        assert_eq!(
            stats.block_cache.capacity,
            1024 * MIB,
            "sizing the row cache must not disturb the block cache"
        );

        // Below the derived floor: an 8 MiB row cache stays 8 MiB.
        let (_tiny_tmp, tiny_provider) = provider_with(Some(8 * MIB), 1024 * MIB);
        let tiny = tiny_provider
            .get_rocksdb_store("")
            .expect("rocksdb store")
            .cache_stats();
        assert_eq!(tiny.row_cache.expect("row cache").capacity, 8 * MIB);
    }

    /// Statistics stay meaningful once a database has served writes and reads.
    #[test]
    fn cache_stats_report_usage_after_writes_and_reads() {
        let (_tmp, provider) = provider_with(Some(32 * MIB), 64 * MIB);
        let store = provider.get_store("").expect("rocksdb store");

        write_keys(&store, 7, 512, 4096);
        store.flush();

        let reader = StoreCache::new_from_store(Arc::clone(&store), false);
        for i in 0..512u32 {
            let key = StorageKey::new(7, i.to_be_bytes().to_vec());
            let item = reader.get(&key).expect("point lookup after flush");
            assert_eq!(item.value_bytes()[0], (i % 251) as u8);
        }

        let stats = store
            .cache_stats()
            .expect("the RocksDB store must expose cache statistics");
        assert_eq!(stats.block_cache.capacity, 64 * MIB);
        assert!(
            stats.block_cache.usage > 0,
            "reads through the block cache must be reflected in usage"
        );
        assert!(
            stats.block_cache.usage <= stats.block_cache.capacity,
            "usage must stay inside the configured capacity"
        );
        assert!(
            stats.block_cache.pinned_usage <= stats.block_cache.usage,
            "pinned entries are a subset of cached entries"
        );
        assert_eq!(stats.row_cache.expect("row cache").capacity, 32 * MIB);
        assert!(
            stats.index_size.is_some(),
            "flushed SST files must report index-block bytes"
        );
        assert!(
            stats.filter_size.is_some(),
            "flushed SST files must report filter-block bytes"
        );
        assert!(
            stats.data_block_size.unwrap_or(0) > 0,
            "flushed SST files must report data-block bytes"
        );
    }

    /// Reads stay correct for every row-cache configuration.
    #[test]
    fn point_lookups_and_scans_survive_each_cache_configuration() {
        for (label, row_cache_size_bytes) in [
            ("disabled", Some(0usize)),
            ("tiny", Some(8 * MIB)),
            ("derived", None),
            ("large", Some(256 * MIB)),
        ] {
            let (_tmp, provider) = provider_with(row_cache_size_bytes, 64 * MIB);
            let store = provider.get_store("").expect("rocksdb store");

            write_keys(&store, 11, 256, 512);
            store.flush();

            let reader = StoreCache::new_from_store(Arc::clone(&store), false);
            for i in 0..256u32 {
                let key = StorageKey::new(11, i.to_be_bytes().to_vec());
                let item = reader
                    .get(&key)
                    .unwrap_or_else(|| panic!("{label}: key {i} must survive"));
                assert_eq!(item.value_bytes()[0], (i % 251) as u8);
            }

            let prefix = StorageKey::new(11, Vec::new());
            let scanned: Vec<StorageKey> = reader
                .find(Some(&prefix), SeekDirection::Forward)
                .map(|(key, item)| {
                    assert_eq!(item.value_bytes().len(), 512, "{label}: scan value width");
                    key
                })
                .collect();
            assert_eq!(
                scanned.len(),
                256,
                "{label}: a forward scan must see every committed row"
            );
            let backwards: Vec<StorageKey> = reader
                .find(Some(&prefix), SeekDirection::Backward)
                .map(|(key, _)| key)
                .collect();
            assert_eq!(
                backwards.len(),
                256,
                "{label}: a backward scan must see every committed row"
            );
            assert_eq!(
                scanned.first(),
                backwards.last(),
                "{label}: the two scan directions must describe the same range"
            );
        }
    }
}
