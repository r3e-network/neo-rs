use super::*;

#[test]
fn property_bytes_reads_the_value_not_the_parenthesised_key() {
    // Real shape of `rocksdb.aggregated-table-properties`: one `; `-separated
    // list of `# name=value` pairs, where the index entry carries digits in
    // its own name.
    let properties = "# data blocks=20; # entries=5120; # data block size=2097152; \
             # index block size (user-key? 1, delta-value? 1)=1234; # filter block size=256;";

    assert_eq!(property_bytes(properties, "data block size"), Some(2097152));
    assert_eq!(
        property_bytes(properties, "index block size"),
        Some(1234),
        "the digits inside `(user-key? 1, ...)` must not be mistaken for the size"
    );
    assert_eq!(property_bytes(properties, "filter block size"), Some(256));
    assert_eq!(property_bytes(properties, "top-level index size"), None);
}

#[test]
fn committed_snapshot_invalidates_latest_state_read_cache() {
    let temp = tempfile::TempDir::new().unwrap();
    let config = StorageConfig {
        path: temp.path().join("cache-commit"),
        ..StorageConfig::default()
    };
    let mut store = RocksDbStore::open(
        &config,
        WriteBatchConfig::default(),
        &Some(ReadCacheConfig::default()),
        false,
        false,
    )
    .unwrap();
    store.enable_read_cache(ReadCacheConfig::default());
    let key = StorageKey::new(1, b"current".to_vec());

    let mut initial = store.snapshot();
    Arc::get_mut(&mut initial)
        .unwrap()
        .put(key.to_array(), b"old".to_vec())
        .unwrap();
    Arc::get_mut(&mut initial).unwrap().try_commit().unwrap();
    assert_eq!(store.try_get(&key).unwrap().value_bytes().as_ref(), b"old");

    let mut pending = store.snapshot();
    Arc::get_mut(&mut pending)
        .unwrap()
        .put(key.to_array(), b"new".to_vec())
        .unwrap();
    // Staging a write must not expose it or evict the valid committed value.
    assert_eq!(store.try_get(&key).unwrap().value_bytes().as_ref(), b"old");
    Arc::get_mut(&mut pending).unwrap().try_commit().unwrap();
    assert_eq!(store.try_get(&key).unwrap().value_bytes().as_ref(), b"new");
}

#[derive(Default)]
struct RecordingProfiler(Mutex<Vec<StoreCommitStage>>);

impl StoreCommitProfiler for RecordingProfiler {
    fn record(&self, stage: StoreCommitStage, _elapsed: std::time::Duration) {
        self.0.lock().push(stage);
    }
}

#[test]
fn store_cache_profiling_observes_commit_without_changing_data() {
    let temp = tempfile::TempDir::new().unwrap();
    let config = StorageConfig {
        path: temp.path().join("commit-profile"),
        ..StorageConfig::default()
    };
    let store = RocksDbStore::open(
        &config,
        WriteBatchConfig::default(),
        &Some(ReadCacheConfig::default()),
        false,
        false,
    )
    .unwrap();
    let store: Arc<dyn Store> = Arc::new(store);
    let profiler = Arc::new(RecordingProfiler::default());
    let mut cache = crate::persistence::StoreCache::new(Arc::clone(&store));
    cache.set_commit_profiler(Some(profiler.clone()));
    let first = StorageKey::new(1, b"first".to_vec());
    cache.add(first.clone(), StorageItem::from_bytes(b"one".to_vec()));

    cache.try_commit().unwrap();

    assert_eq!(
        store.try_get(&first).unwrap().value_bytes().as_ref(),
        b"one"
    );
    assert_eq!(
        profiler.0.lock().as_slice(),
        &[
            StoreCommitStage::WriterSnapshotOpen,
            StoreCommitStage::ChangeApplication,
            StoreCommitStage::WriteBatchBuild,
            StoreCommitStage::DatabaseWrite,
            StoreCommitStage::CacheInvalidation,
        ]
    );

    profiler.0.lock().clear();
    cache.set_commit_profiler(None);
    let second = StorageKey::new(1, b"second".to_vec());
    cache.add(second.clone(), StorageItem::from_bytes(b"two".to_vec()));
    cache.try_commit().unwrap();
    assert_eq!(
        store.try_get(&second).unwrap().value_bytes().as_ref(),
        b"two"
    );
    assert!(profiler.0.lock().is_empty());
    store.try_flush_durable().unwrap();
    assert_eq!(
        store.try_get(&first).unwrap().value_bytes().as_ref(),
        b"one"
    );
    assert_eq!(
        store.try_get(&second).unwrap().value_bytes().as_ref(),
        b"two"
    );
}
