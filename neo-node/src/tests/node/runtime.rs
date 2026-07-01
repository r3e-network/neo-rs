use super::*;

/// `commit_to_store` flushes the writes accumulated in the shared snapshot
/// (as a block's native-persist pipeline does) through to the durable store,
/// so a fresh cache over the same store reads them. Without this, synced
/// blocks stay in-memory and the on-disk tip is stuck at genesis.
#[test]
fn commit_to_store_flushes_snapshot_writes_to_durable_store() {
    use neo_blockchain::service_context::SystemContext;
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::persistence::{StoreCache, store::Store};
    use neo_storage::{StorageItem, StorageKey};

    let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(Arc::clone(&store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    let ctx = DaemonContext::new(
        Arc::new(ProtocolSettings::default()),
        Arc::clone(&snapshot),
        store_cache,
        None,
        false,
        None,
        None,
    );

    // Stage a write into the shared snapshot (the blockchain persist path).
    let key = StorageKey::new(-1, vec![0xAB, 0xCD]);
    snapshot.add(key.clone(), StorageItem::from_bytes(vec![0x01, 0x02, 0x03]));

    // Not durable yet: a fresh cache over the same store cannot see it.
    let before = StoreCache::new_from_store(Arc::clone(&store), false);
    assert!(
        before.data_cache().get(&key).is_none(),
        "write must not reach the store before commit_to_store"
    );

    // Flush, then a fresh cache over the same store reads the write.
    ctx.commit_to_store();
    let after = StoreCache::new_from_store(Arc::clone(&store), false);
    assert!(
        after.data_cache().get(&key).is_some(),
        "commit_to_store must flush the snapshot write through to the store"
    );
}

#[test]
fn daemon_context_skips_state_service_mpt_during_default_cold_catchup() {
    use neo_blockchain::service_context::{BlockPersistContext, SystemContext};
    use neo_payloads::{Block, Header};
    use neo_state_service::{StateStore, commit_handlers::StateServiceCommitHandlers};
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::persistence::{StoreCache, store::Store};

    let chain_store: Arc<dyn Store> = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(Arc::clone(&chain_store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    let state_store = Arc::new(StateStore::with_mpt(true));
    let state_service = Arc::new(StateServiceCommitHandlers::new(Arc::clone(&state_store)));
    let ctx = DaemonContext::new(
        Arc::new(ProtocolSettings::default()),
        Arc::clone(&snapshot),
        store_cache,
        Some(state_service),
        false,
        None,
        None,
    );

    let mut header = Header::new();
    header.set_index(0);
    let block = Block::from_parts(header, Vec::new());

    assert!(ctx.block_committing_with_live_tip(&block, &snapshot, &[], 1_000_000));
    assert_eq!(
        state_store
            .mpt()
            .expect("state store exposes MPT")
            .current_local_root_index(),
        None,
        "default cold catch-up should preserve fast-sync behavior by deferring StateService MPT"
    );

    let mut header = Header::new();
    header.set_index(1);
    let block = Block::from_parts(header, Vec::new());

    assert!(ctx.block_committing_with_context(
        &block,
        &snapshot,
        &[],
        BlockPersistContext::bulk_sync(),
    ));
    assert_eq!(
        state_store
            .mpt()
            .expect("state store exposes MPT")
            .current_local_root_index(),
        None,
        "explicit bulk import should stay on the cold-catchup fast path before peers report a live tip"
    );
}

#[test]
fn daemon_context_can_track_state_service_mpt_during_cold_catchup_for_validation() {
    use neo_blockchain::service_context::{BlockPersistContext, SystemContext};
    use neo_payloads::{Block, Header};
    use neo_state_service::{StateStore, commit_handlers::StateServiceCommitHandlers};
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::persistence::{StoreCache, store::Store};

    let chain_store: Arc<dyn Store> = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(Arc::clone(&chain_store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    let state_store = Arc::new(StateStore::with_mpt(true));
    let state_service = Arc::new(StateServiceCommitHandlers::new(Arc::clone(&state_store)));
    let ctx = DaemonContext::new(
        Arc::new(ProtocolSettings::default()),
        Arc::clone(&snapshot),
        store_cache,
        Some(state_service),
        true,
        None,
        None,
    );

    let mut header = Header::new();
    header.set_index(0);
    let block = Block::from_parts(header, Vec::new());

    assert!(ctx.block_committing_with_live_tip(&block, &snapshot, &[], 1_000_000));
    let mpt = state_store.mpt().expect("state store exposes MPT");
    assert_eq!(mpt.current_local_root_index(), Some(0));
    assert!(
        mpt.get_state_root(0).is_some(),
        "validation profiles must keep local state roots advancing during cold catch-up"
    );

    let mut header = Header::new();
    header.set_index(1);
    let block = Block::from_parts(header, Vec::new());

    assert!(ctx.block_committing_with_context(
        &block,
        &snapshot,
        &[],
        BlockPersistContext::bulk_sync(),
    ));
    let mpt = state_store.mpt().expect("state store exposes MPT");
    assert_eq!(
        mpt.current_local_root_index(),
        Some(1),
        "validation bulk import must still advance StateService MPT roots"
    );
}

#[test]
fn state_service_store_uses_fast_sync_for_validation_import() {
    let temp = tempfile::tempdir().expect("temp StateService root");
    let path = temp.path().join("StateRoot_{0}");
    let store = services::open_service_store("StateService", "rocksdb", &path, 0x334F_454E, true)
        .expect("open fast-sync StateService store");

    let rocksdb = store
        .as_any()
        .downcast_ref::<neo_storage::rocksdb::RocksDbStore>()
        .expect("service store uses RocksDB");

    assert!(
        rocksdb.write_batch_config().disable_wal,
        "StateService fast-sync import should disable WAL like the chain store"
    );
}

#[test]
fn configured_mdbx_backend_is_used_for_service_stores() {
    let temp = tempfile::tempdir().expect("temp service store root");
    let config: NodeConfig = toml::from_str(&format!(
        r#"
[storage]
backend = "mdbx"

[state_service]
enabled = true
path = "{}"
"#,
        temp.path().join("StateRoot_{{0}}").display(),
    ))
    .expect("parse mdbx service config");

    let services = services::build_operational_services(&config, 0x334F_454E, true, false)
        .expect("build services");
    let state_store = services
        .durable_stores
        .first()
        .expect("state service durable store");

    assert!(state_store.as_any().is::<neo_storage::mdbx::MdbxStore>());
}

#[test]
fn operational_state_service_store_keeps_wal_for_normal_restart() {
    let temp = tempfile::tempdir().expect("temp StateService root");
    let config: NodeConfig = toml::from_str(&format!(
        r#"
[storage]
backend = "rocksdb"

[state_service]
enabled = true
path = "{}"
"#,
        temp.path().join("StateRoot_{{0}}").display(),
    ))
    .expect("parse state-service config");

    let services = services::build_operational_services(&config, 0x334F_454E, true, false)
        .expect("build services");
    let state_store = services
        .durable_stores
        .first()
        .expect("state service durable store");
    let rocksdb = state_store
        .as_any()
        .downcast_ref::<neo_storage::rocksdb::RocksDbStore>()
        .expect("state service uses RocksDB");

    assert!(
        !rocksdb.write_batch_config().disable_wal,
        "normal restart must not leave StateService RocksDB in fast-sync mode"
    );
    assert!(
        !services
            .state_service
            .as_ref()
            .expect("state service enabled")
            .is_async(),
        "normal replay should keep StateService MPT failures synchronous"
    );
}

#[test]
fn fast_sync_state_service_uses_ordered_async_mpt_worker() {
    let temp = tempfile::tempdir().expect("temp StateService root");
    let config: NodeConfig = toml::from_str(&format!(
        r#"
[storage]
backend = "rocksdb"

[state_service]
enabled = true
path = "{}"
track_during_catchup = true
"#,
        temp.path().join("StateRoot_{{0}}").display(),
    ))
    .expect("parse state-service config");

    let services = services::build_operational_services(&config, 0x334F_454E, true, true)
        .expect("build fast-sync services");
    let state_service = services
        .state_service
        .as_ref()
        .expect("state service enabled");

    assert!(
        state_service.is_async(),
        "fast-sync validation should overlap native persistence with ordered StateService MPT writes"
    );
}

#[test]
fn daemon_context_bulk_sync_flush_reports_async_state_service_failure() {
    use neo_blockchain::service_context::{BlockPersistContext, SystemContext};
    use neo_payloads::{Block, Header};
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::persistence::{StoreCache, store::Store};
    use neo_storage::{StorageItem, StorageKey};

    let config: NodeConfig = toml::from_str(
        r#"
[state_service]
enabled = true
full_state = true
track_during_catchup = true
"#,
    )
    .expect("parse validation state-service config");
    let services = services::build_operational_services(&config, 0x334F_454E, true, true)
        .expect("build fast-sync validation services");
    let state_service = services
        .state_service
        .as_ref()
        .expect("state service enabled")
        .clone();
    assert!(
        state_service.is_async(),
        "fixture must exercise the async StateService worker"
    );

    let chain_store: Arc<dyn Store> = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(Arc::clone(&chain_store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    snapshot.add(
        StorageKey::new(5, vec![0xAA]),
        StorageItem::from_bytes(vec![0x01]),
    );
    let ctx = DaemonContext::new(
        Arc::new(ProtocolSettings::default()),
        Arc::clone(&snapshot),
        store_cache,
        Some(state_service),
        true,
        None,
        None,
    );

    let mut header = Header::new();
    header.set_index(5);
    let block = Block::from_parts(header, Vec::new());

    assert!(
        ctx.block_committing_with_context(&block, &snapshot, &[], BlockPersistContext::bulk_sync()),
        "async StateService enqueue succeeds before the worker observes the non-contiguous root"
    );
    let err = ctx
        .flush_bulk_sync_commit_handlers()
        .expect_err("bulk-sync finalization must surface async StateService worker failure");
    assert!(
        err.contains("state-root worker"),
        "unexpected flush error: {err}"
    );
}

#[test]
fn daemon_context_live_async_state_service_failure_is_immediate() {
    use neo_blockchain::service_context::{BlockPersistContext, SystemContext};
    use neo_payloads::{Block, Header};
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::persistence::{StoreCache, store::Store};
    use neo_storage::{StorageItem, StorageKey};

    let config: NodeConfig = toml::from_str(
        r#"
[state_service]
enabled = true
full_state = true
track_during_catchup = true
"#,
    )
    .expect("parse validation state-service config");
    let services = services::build_operational_services(&config, 0x334F_454E, true, true)
        .expect("build fast-sync validation services");
    let state_service = services
        .state_service
        .as_ref()
        .expect("state service enabled")
        .clone();
    assert!(
        state_service.is_async(),
        "fixture must exercise the async StateService worker"
    );

    let chain_store: Arc<dyn Store> = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(Arc::clone(&chain_store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    snapshot.add(
        StorageKey::new(5, vec![0xAA]),
        StorageItem::from_bytes(vec![0x01]),
    );
    let ctx = DaemonContext::new(
        Arc::new(ProtocolSettings::default()),
        Arc::clone(&snapshot),
        store_cache,
        Some(state_service),
        true,
        None,
        None,
    );

    let mut header = Header::new();
    header.set_index(5);
    let block = Block::from_parts(header, Vec::new());

    assert!(
        !ctx.block_committing_with_context(&block, &snapshot, &[], BlockPersistContext::live()),
        "live async StateService must fail before chain commit when MPT roots are non-contiguous"
    );
}

#[test]
fn fast_sync_state_service_without_catchup_tracking_stays_synchronous() {
    let temp = tempfile::tempdir().expect("temp StateService root");
    let config: NodeConfig = toml::from_str(&format!(
        r#"
[storage]
backend = "rocksdb"

[state_service]
enabled = true
path = "{}"
track_during_catchup = false
"#,
        temp.path().join("StateRoot_{{0}}").display(),
    ))
    .expect("parse state-service config");

    let services = services::build_operational_services(&config, 0x334F_454E, true, true)
        .expect("build fast-sync services");
    let state_service = services
        .state_service
        .as_ref()
        .expect("state service enabled");

    assert!(
        !state_service.is_async(),
        "fast-sync store mode alone should not pay async StateService overhead when catch-up MPT tracking is disabled"
    );

    let state_store = services
        .durable_stores
        .first()
        .expect("state service durable store");
    let rocksdb = state_store
        .as_any()
        .downcast_ref::<neo_storage::rocksdb::RocksDbStore>()
        .expect("state service uses RocksDB");
    assert!(
        !rocksdb.write_batch_config().disable_wal,
        "StateService RocksDB should stay in durable mode when catch-up MPT tracking is disabled"
    );
}

#[tokio::test]
async fn build_node_uses_fast_sync_store_mode_for_resumed_startup_import() {
    use neo_storage::persistence::storage::StorageConfig;
    use neo_storage::rocksdb::{RocksDBStoreProvider, RocksDbStore};

    const DURABLE_TIP: u32 = 1;

    let temp = tempfile::tempdir().expect("temp RocksDB root");
    let storage_path = temp.path().join("chain");
    let state_path_template = temp.path().join("StateRoot_{0}");
    let settings = Arc::new(ProtocolSettings::default());
    seed_rocksdb_tip(&storage_path, settings.as_ref(), DURABLE_TIP)
        .expect("seed durable RocksDB tip");

    let state_path = temp
        .path()
        .join(format!("StateRoot_{:08X}", settings.network));
    let provider = RocksDBStoreProvider::new(StorageConfig {
        path: state_path,
        ..StorageConfig::default()
    });
    let state_store = provider.get_store("").expect("open state store");
    let mut state_snapshot = state_store.snapshot();
    let state_writer = Arc::get_mut(&mut state_snapshot).expect("exclusive state snapshot");
    state_writer
        .put(vec![0x02], DURABLE_TIP.to_le_bytes().to_vec())
        .expect("write current root index");
    state_writer.try_commit().expect("commit state root height");
    drop(state_snapshot);
    drop(state_store);
    drop(provider);

    let mut config: NodeConfig = toml::from_str(
        r#"
[storage]
backend = "rocksdb"
"#,
    )
    .expect("parse rocksdb storage config");
    config.state_service.enabled = true;
    config.state_service.track_during_catchup = true;
    config.state_service.path = Some(state_path_template);

    let running = build_node(
        Arc::clone(&settings),
        &config,
        Some(&storage_path),
        None,
        LedgerMode::Local,
        true,
        None,
    )
    .await
    .expect("build node for resumed startup import");

    let chain_store = running.node.storage();
    let chain_rocksdb = chain_store
        .as_any()
        .downcast_ref::<RocksDbStore>()
        .expect("chain store uses RocksDB");
    assert!(
        chain_rocksdb.write_batch_config().disable_wal,
        "resumed startup import should keep chain RocksDB in fast-sync mode even when the durable tip is nonzero"
    );

    let service_store = running
        .durable_service_stores
        .first()
        .expect("state service durable store");
    let service_rocksdb = service_store
        .as_any()
        .downcast_ref::<RocksDbStore>()
        .expect("state service uses RocksDB");
    assert!(
        service_rocksdb.write_batch_config().disable_wal,
        "resumed startup import should put StateService RocksDB in fast-sync mode too"
    );

    for handle in running.handles {
        handle.abort();
        let _ = handle.await;
    }
    drop(running.node);
    drop(running.network);
}

#[test]
fn validation_state_service_reports_non_contiguous_root_before_chain_commit() {
    use neo_payloads::{Block, Header};
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::persistence::{StoreCache, store::Store};
    use neo_storage::{StorageItem, StorageKey};

    let config: NodeConfig = toml::from_str(
        r#"
[state_service]
enabled = true
full_state = true
track_during_catchup = true
"#,
    )
    .expect("parse validation state-service config");
    let services = services::build_operational_services(&config, 0x334F_454E, true, false)
        .expect("build validation services");
    let state_store = services
        .state_store
        .as_ref()
        .expect("state store enabled")
        .clone();
    let state_service = services
        .state_service
        .as_ref()
        .expect("state service enabled")
        .clone();

    let chain_store: Arc<dyn Store> = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(Arc::clone(&chain_store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    snapshot.add(
        StorageKey::new(5, vec![0xAA]),
        StorageItem::from_bytes(vec![0x01]),
    );
    let ctx = DaemonContext::new(
        Arc::new(ProtocolSettings::default()),
        Arc::clone(&snapshot),
        store_cache,
        Some(state_service),
        true,
        None,
        None,
    );

    let mut header = Header::new();
    header.set_index(5);
    let block = Block::from_parts(header, Vec::new());

    assert!(
        !ctx.block_committing_with_live_tip(&block, &snapshot, &[], 1_000_000),
        "validation StateService must fail before chain commit when MPT roots would become non-contiguous"
    );
    let mpt = state_store.mpt().expect("state store exposes MPT");
    assert_eq!(mpt.current_local_root_index(), None);
    assert!(mpt.get_state_root(5).is_none());
}

#[test]
fn default_state_service_reports_near_tip_root_failure_before_chain_commit() {
    use neo_payloads::{Block, Header};
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::persistence::{StoreCache, store::Store};
    use neo_storage::{StorageItem, StorageKey};

    let config: NodeConfig = toml::from_str(
        r#"
[state_service]
enabled = true
full_state = true
"#,
    )
    .expect("parse default state-service config");
    let services = services::build_operational_services(&config, 0x334F_454E, true, false)
        .expect("build services");
    let state_store = services
        .state_store
        .as_ref()
        .expect("state store enabled")
        .clone();
    let state_service = services
        .state_service
        .as_ref()
        .expect("state service enabled")
        .clone();

    let chain_store: Arc<dyn Store> = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(Arc::clone(&chain_store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    snapshot.add(
        StorageKey::new(5, vec![0xBB]),
        StorageItem::from_bytes(vec![0x02]),
    );
    let ctx = DaemonContext::new(
        Arc::new(ProtocolSettings::default()),
        Arc::clone(&snapshot),
        store_cache,
        Some(state_service),
        false,
        None,
        None,
    );

    let mut header = Header::new();
    header.set_index(5);
    let block = Block::from_parts(header, Vec::new());

    assert!(
        !ctx.block_committing_with_live_tip(&block, &snapshot, &[], 0),
        "when default StateService tracking is active, MPT failures must stop chain commit"
    );
    let mpt = state_store.mpt().expect("state store exposes MPT");
    assert_eq!(mpt.current_local_root_index(), None);
    assert!(mpt.get_state_root(5).is_none());
}

#[test]
fn restore_durable_store_mode_reenables_wal_for_chain_and_service_stores() {
    use neo_storage::persistence::storage::StorageConfig;
    use neo_storage::persistence::store::Store;
    use neo_storage::rocksdb::{RocksDBStoreProvider, RocksDbStore};

    let temp = tempfile::tempdir().expect("temp store root");
    let chain_cfg = StorageConfig {
        path: temp.path().join("chain"),
        ..Default::default()
    };
    let chain_store = Arc::new(
        RocksDBStoreProvider::new(chain_cfg)
            .get_rocksdb_store("")
            .expect("chain RocksDB store"),
    );
    chain_store.enable_fast_sync_mode();
    let chain_store_trait: Arc<dyn Store> = chain_store.clone();

    let state_path = temp.path().join("StateRoot_{0}");
    let service_store =
        services::open_service_store("StateService", "rocksdb", &state_path, 0x334F_454E, true)
            .expect("open fast-sync StateService store");

    restore_durable_store_mode(chain_store_trait.as_ref(), &[Arc::clone(&service_store)])
        .expect("restore durable mode");

    assert!(
        !chain_store.write_batch_config().disable_wal,
        "chain store must restore WAL before normal node operation"
    );
    let service_rocksdb = service_store
        .as_any()
        .downcast_ref::<RocksDbStore>()
        .expect("service store uses RocksDB");
    assert!(
        !service_rocksdb.write_batch_config().disable_wal,
        "StateService store must restore WAL with the chain store"
    );
}

#[test]
fn abort_fast_sync_store_mode_discards_pending_chain_and_service_writes() {
    use neo_storage::persistence::StoreCache;
    use neo_storage::persistence::storage::StorageConfig;
    use neo_storage::persistence::store::Store;
    use neo_storage::rocksdb::RocksDBStoreProvider;
    use neo_storage::{StorageItem, StorageKey};

    let temp = tempfile::tempdir().expect("temp store root");
    let chain_cfg = StorageConfig {
        path: temp.path().join("chain"),
        ..Default::default()
    };
    let chain_store = Arc::new(
        RocksDBStoreProvider::new(chain_cfg)
            .get_rocksdb_store("")
            .expect("chain RocksDB store"),
    );
    chain_store.enable_fast_sync_mode();
    let chain_store_trait: Arc<dyn Store> = chain_store.clone();

    let state_path = temp.path().join("StateRoot_{0}");
    let service_store =
        services::open_service_store("StateService", "rocksdb", &state_path, 0x334F_454E, true)
            .expect("open fast-sync StateService store");

    let chain_key = StorageKey::new(77, b"partial-chain-block".to_vec());
    let service_key = StorageKey::new(88, b"partial-state-root".to_vec());
    let mut chain_writer = StoreCache::new_from_store(chain_store.clone(), false);
    chain_writer.add(chain_key.clone(), StorageItem::from_bytes(vec![0xC1]));
    chain_writer
        .try_commit()
        .expect("chain fast-sync write buffers");
    let mut service_writer = StoreCache::new_from_store(Arc::clone(&service_store), false);
    service_writer.add(service_key.clone(), StorageItem::from_bytes(vec![0x51]));
    service_writer
        .try_commit()
        .expect("service fast-sync write buffers");

    abort_fast_sync_store_mode(chain_store_trait.as_ref(), &[Arc::clone(&service_store)]);
    chain_store.flush().expect("chain cleanup flush");
    service_store.flush().expect("service cleanup flush");

    let chain_reader = StoreCache::new_from_store(chain_store, false);
    assert!(
        chain_reader.get(&chain_key).is_none(),
        "failed startup import cleanup must not persist partial chain writes"
    );
    let service_reader = StoreCache::new_from_store(service_store, false);
    assert!(
        service_reader.get(&service_key).is_none(),
        "failed startup import cleanup must not persist partial service-store writes"
    );
}

#[tokio::test]
async fn build_node_rejects_missing_state_root_store_for_populated_chain() {
    const DURABLE_TIP: u32 = 1;

    let temp = tempfile::tempdir().expect("temp RocksDB root");
    let storage_path = temp.path().join("chain");
    let state_path_template = temp.path().join("StateRoot_{0}");
    let settings = Arc::new(ProtocolSettings::default());
    seed_rocksdb_tip(&storage_path, settings.as_ref(), DURABLE_TIP)
        .expect("seed durable RocksDB tip");

    let mut config: NodeConfig = toml::from_str(
        r#"
[storage]
backend = "rocksdb"
"#,
    )
    .expect("parse rocksdb storage config");
    config.state_service.enabled = true;
    config.state_service.path = Some(state_path_template.clone());

    let err = match build_node(
        Arc::clone(&settings),
        &config,
        Some(&storage_path),
        None,
        LedgerMode::Local,
        false,
        None,
    )
    .await
    {
        Ok(running) => {
            for handle in running.handles {
                handle.abort();
                let _ = handle.await;
            }
            panic!("missing StateRoot store must abort local node startup");
        }
        Err(err) => err,
    };

    assert!(
        err.to_string().contains("StateService MPT store"),
        "startup error should name StateRoot parity failure: {err:#}"
    );
    assert!(
        !temp
            .path()
            .join(format!("StateRoot_{:08X}", settings.network))
            .exists(),
        "startup guard must run before creating a fresh mismatched StateRoot store"
    );
}

#[tokio::test]
async fn remote_ledger_node_build_does_not_register_local_replay_services() {
    let temp = tempfile::tempdir().expect("temp stores");
    let settings = Arc::new(ProtocolSettings::default());
    let config: NodeConfig = toml::from_str(&format!(
        r#"
[state_service]
enabled = true
path = "{}"

[indexer]
enabled = true
store_path = "{}"

[application_logs]
enabled = true
path = "{}"

[tokens_tracker]
enabled = true
db_path = "{}"
"#,
        temp.path().join("StateRoot_{{0}}").display(),
        temp.path().join("NeoIndexer_{{0}}").display(),
        temp.path().join("ApplicationLogs_{{0}}").display(),
        temp.path().join("TokensTracker_{{0}}").display(),
    ))
    .expect("parse service config");

    let running = build_node(
        Arc::clone(&settings),
        &config,
        Some(&temp.path().join("chain")),
        None,
        LedgerMode::RemoteRpc {
            endpoint: "https://rpc.example.invalid",
        },
        false,
        None,
    )
    .await
    .expect("build remote-ledger node");

    assert!(
        running
            .node
            .get_service::<neo_state_service::commit_handlers::StateServiceCommitHandlers>()
            .is_none(),
        "remote-ledger mode must not start local StateService replay"
    );
    assert!(
        running
            .node
            .get_service::<neo_indexer::IndexerService>()
            .is_none(),
        "remote-ledger mode must not start local indexer replay"
    );
    assert!(
        running
            .node
            .get_service::<neo_rpc::application_logs::ApplicationLogsService>()
            .is_none(),
        "remote-ledger mode must not start local application-log replay"
    );
    assert!(
        running
            .node
            .get_service::<neo_rpc::plugins::tokens_tracker::TokensTrackerService>()
            .is_none(),
        "remote-ledger mode must not start local token tracker replay"
    );
    assert!(
        running
            .node
            .storage()
            .as_any()
            .downcast_ref::<neo_storage::persistence::providers::memory_store::MemoryStore>()
            .is_some(),
        "remote-ledger mode should use an ephemeral chain context instead of opening the configured local ledger"
    );
    assert!(
        !temp.path().join("chain").exists(),
        "remote-ledger mode must not create a local chain RocksDB directory"
    );
    assert!(
        neo_native_contracts::LedgerContract::new()
            .current_index(running.node.store_cache().data_cache())
            .is_err(),
        "remote-ledger mode must not initialize even an ephemeral canonical ledger"
    );

    for handle in running.handles {
        handle.abort();
        let _ = handle.await;
    }
    drop(running.node);
    drop(running.network);
}

#[tokio::test]
async fn remote_ledger_node_advertises_upstream_height_when_available() {
    const REMOTE_BLOCK_COUNT: u32 = 42;

    let temp = tempfile::tempdir().expect("temp stores");
    let settings = Arc::new(ProtocolSettings::default());
    let endpoint = serve_rpc_once("getblockcount", serde_json::json!(REMOTE_BLOCK_COUNT));
    let running = build_node(
        Arc::clone(&settings),
        &NodeConfig::default(),
        Some(&temp.path().join("chain")),
        None,
        LedgerMode::RemoteRpc {
            endpoint: endpoint.as_str(),
        },
        false,
        None,
    )
    .await
    .expect("build remote-ledger node");

    running
        .network
        .start("127.0.0.1:0".parse().unwrap())
        .await
        .expect("start P2P listener");
    let port = running.network.local_node_info().port();
    let mut fake = fake_dial(port).await;
    let node_version = recv_frame(&mut fake).await;
    assert_eq!(node_version.command, MessageCommand::Version);
    let node_version: VersionPayload = decode_payload(&node_version);
    assert!(
        node_version.capabilities.iter().any(|capability| matches!(
            capability,
            NodeCapability::FullNode {
                start_height: height
            } if *height == REMOTE_BLOCK_COUNT - 1
        )),
        "remote-ledger mode should advertise the upstream RPC tip height"
    );

    running.network.shutdown().await.expect("shutdown network");
    for handle in running.handles {
        handle.abort();
        let _ = handle.await;
    }
    drop(running.node);
    drop(running.network);
}

/// Reproduces the v3.10.0 consistency testnet failure
/// (`Policy_getExecFeeFactor`: Python 30 vs local node 0).
///
/// The live RPC `invokefunction(Policy.getExecFeeFactor)` path builds its
/// engine over a FRESH `system.store_cache()` (see Session::new), NOT over
/// the genesis-persist shared snapshot. So this test runs the genesis
/// native-persist pipeline into the shared snapshot, commits via
/// `commit_to_store`, then reads Policy storage through a brand-new
/// `store_cache` view — exactly the live read path. If the genesis
/// `ExecFeeFactor=30` write is visible there, the live node must return 30.
#[test]
fn genesis_policy_init_visible_through_fresh_store_cache_after_commit() {
    use neo_blockchain::native_persist::{
        chain_state_initialized, genesis_block, persist_block_natives,
    };
    use neo_blockchain::service_context::SystemContext;
    use neo_native_contracts::PolicyContract;
    use neo_storage::StorageKey;
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::persistence::{StoreCache, store::Store};
    use num_bigint::BigInt;

    neo_native_contracts::install();
    let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(Arc::clone(&store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    let ctx = DaemonContext::new(
        Arc::new(ProtocolSettings::default()),
        Arc::clone(&snapshot),
        store_cache,
        None,
        false,
        None,
        None,
    );

    // Sanity: empty store is not yet initialized.
    assert!(!chain_state_initialized(&snapshot));

    // Run the genesis native-persist pipeline into the shared snapshot, then
    // flush to the durable store — exactly handlers.rs::initialize().
    let settings = ProtocolSettings::default();
    let genesis = Arc::new(genesis_block(&settings).expect("genesis block"));
    persist_block_natives(Arc::clone(&snapshot), Arc::clone(&genesis), &settings)
        .expect("genesis persist");
    ctx.commit_to_store();

    // The live RPC read path: a FRESH store_cache over the same backing store.
    let read_cache = StoreCache::new_from_store(Arc::clone(&store), false);
    // Policy ExecFeeFactor prefix (PolicyContract.cs:93 Prefix_ExecFeeFactor = 18).
    let key = StorageKey::new(PolicyContract::ID, vec![18u8]);
    let value = read_cache
        .data_cache()
        .get(&key)
        .map(|item| item.value_bytes().into_owned());
    assert_eq!(
        value,
        Some(BigInt::from(30i64).to_signed_bytes_le()),
        "Policy ExecFeeFactor=30 must be visible through a fresh store_cache \
         after genesis persist + commit_to_store (the live RPC read path). \
         This is the v3.10.0 Policy_getExecFeeFactor parity requirement."
    );
}

#[test]
fn daemon_context_indexes_application_executed_notifications() {
    use neo_blockchain::service_context::SystemContext;
    use neo_payloads::{ApplicationExecuted, Block, Header, NotifyEventArgs, Signer, Transaction};
    use neo_primitives::{TriggerType, UInt160, WitnessScope};
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::persistence::{StoreCache, store::Store};
    use neo_vm_rs::VmState as VMState;

    let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(Arc::clone(&store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    let indexer = Arc::new(neo_indexer::IndexerService::new());
    let ctx = DaemonContext::new(
        Arc::new(ProtocolSettings::default()),
        Arc::clone(&snapshot),
        store_cache,
        None,
        false,
        Some(Arc::clone(&indexer)),
        None,
    );

    let signer = UInt160::from_bytes(&[1; UInt160::LENGTH]).expect("signer");
    let contract = UInt160::from_bytes(&[2; UInt160::LENGTH]).expect("contract");
    let mut tx = Transaction::new();
    tx.set_nonce(91);
    tx.set_script(vec![0x51]);
    tx.set_signers(vec![Signer::new(signer, WitnessScope::CALLED_BY_ENTRY)]);
    let tx_hash = tx.try_hash().expect("tx hash");

    let mut header = Header::new();
    header.set_index(5);
    let mut block = Block::from_parts(header, vec![tx.clone()]);
    block.try_rebuild_merkle_root().expect("merkle root");

    let mut executed = ApplicationExecuted::new(
        Some(tx),
        TriggerType::APPLICATION,
        VMState::HALT,
        None,
        0,
        Vec::new(),
    );
    executed
        .notifications
        .push(NotifyEventArgs::new_with_optional_container(
            None,
            contract,
            "Transfer".to_string(),
            Vec::new(),
        ));

    assert!(ctx.block_committing(&block, &snapshot, &[executed]));

    let records = indexer.notifications_for_transaction(&tx_hash, 0, 10);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].contract_hash, contract);
    assert_eq!(records[0].event_name, "Transfer");
    assert_eq!(records[0].block_height, 5);
}

#[test]
fn daemon_context_dispatches_application_logs_handlers() {
    use neo_blockchain::service_context::SystemContext;
    use neo_payloads::{ApplicationExecuted, Block, Header, NotifyEventArgs, Signer, Transaction};
    use neo_primitives::{TriggerType, UInt160, WitnessScope};
    use neo_rpc::application_logs::{ApplicationLogsService, ApplicationLogsSettings};
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::persistence::{StoreCache, store::Store};
    use neo_vm_rs::VmState as VMState;

    let settings = Arc::new(ProtocolSettings::default());
    let chain_store: Arc<dyn Store> = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(Arc::clone(&chain_store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());

    let mut logs_settings = ApplicationLogsSettings::default();
    logs_settings.enabled = true;
    logs_settings.network = settings.network;
    let logs_service = Arc::new(ApplicationLogsService::new(
        logs_settings,
        Arc::new(MemoryStore::new()),
    ));

    let ctx = DaemonContext::new(
        Arc::clone(&settings),
        Arc::clone(&snapshot),
        store_cache,
        None,
        false,
        None,
        Some(Arc::clone(&logs_service)),
    );
    let node = Arc::new(neo_system::Node::new(settings, None, None).expect("node"));
    ctx.set_node(node);

    let signer = UInt160::from_bytes(&[5; UInt160::LENGTH]).expect("signer");
    let contract = UInt160::from_bytes(&[6; UInt160::LENGTH]).expect("contract");
    let mut tx = Transaction::new();
    tx.set_nonce(117);
    tx.set_script(vec![0x51]);
    tx.set_signers(vec![Signer::new(signer, WitnessScope::CALLED_BY_ENTRY)]);
    let tx_hash = tx.try_hash().expect("tx hash");

    let mut header = Header::new();
    header.set_index(7);
    let mut block = Block::from_parts(header, vec![tx.clone()]);
    block.try_rebuild_merkle_root().expect("merkle root");

    let mut executed = ApplicationExecuted::new(
        Some(tx),
        TriggerType::APPLICATION,
        VMState::HALT,
        None,
        10,
        Vec::new(),
    );
    executed
        .notifications
        .push(NotifyEventArgs::new_with_optional_container(
            None,
            contract,
            "Transfer".to_string(),
            Vec::new(),
        ));

    assert!(ctx.block_committing(&block, &snapshot, &[executed]));
    ctx.block_committed(&block);

    let tx_log = logs_service
        .get_transaction_log(&tx_hash)
        .expect("transaction application log");
    assert_eq!(tx_log["txid"], tx_hash.to_string());
    assert_eq!(tx_log["executions"][0]["trigger"], "Application");
    assert_eq!(
        tx_log["executions"][0]["notifications"][0]["eventname"],
        "Transfer"
    );
}

#[test]
fn daemon_context_skips_application_logs_committed_handler_during_bulk_sync() {
    use neo_blockchain::service_context::{BlockPersistContext, SystemContext};
    use neo_payloads::{ApplicationExecuted, Block, Header, NotifyEventArgs, Signer, Transaction};
    use neo_primitives::{TriggerType, UInt160, WitnessScope};
    use neo_rpc::application_logs::{ApplicationLogsService, ApplicationLogsSettings};
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::persistence::{StoreCache, store::Store};
    use neo_vm_rs::VmState as VMState;

    let settings = Arc::new(ProtocolSettings::default());
    let chain_store: Arc<dyn Store> = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(Arc::clone(&chain_store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());

    let mut logs_settings = ApplicationLogsSettings::default();
    logs_settings.enabled = true;
    logs_settings.network = settings.network;
    let logs_service = Arc::new(ApplicationLogsService::new(
        logs_settings,
        Arc::new(MemoryStore::new()),
    ));

    let ctx = DaemonContext::new(
        Arc::clone(&settings),
        Arc::clone(&snapshot),
        store_cache,
        None,
        false,
        None,
        Some(Arc::clone(&logs_service)),
    );
    let node = Arc::new(neo_system::Node::new(settings, None, None).expect("node"));
    ctx.set_node(node);

    let signer = UInt160::from_bytes(&[7; UInt160::LENGTH]).expect("signer");
    let contract = UInt160::from_bytes(&[8; UInt160::LENGTH]).expect("contract");
    let mut tx = Transaction::new();
    tx.set_nonce(223);
    tx.set_script(vec![0x51]);
    tx.set_signers(vec![Signer::new(signer, WitnessScope::CALLED_BY_ENTRY)]);
    let tx_hash = tx.try_hash().expect("tx hash");

    let mut header = Header::new();
    header.set_index(11);
    let mut block = Block::from_parts(header, vec![tx.clone()]);
    block.try_rebuild_merkle_root().expect("merkle root");

    let mut executed = ApplicationExecuted::new(
        Some(tx),
        TriggerType::APPLICATION,
        VMState::HALT,
        None,
        10,
        Vec::new(),
    );
    executed
        .notifications
        .push(NotifyEventArgs::new_with_optional_container(
            None,
            contract,
            "Transfer".to_string(),
            Vec::new(),
        ));

    assert!(ctx.block_committing_with_context(
        &block,
        &snapshot,
        &[executed],
        BlockPersistContext::bulk_sync(),
    ));
    ctx.block_committed_with_context(&block, BlockPersistContext::bulk_sync());

    assert!(
        logs_service.get_transaction_log(&tx_hash).is_none(),
        "bulk sync must not commit local ApplicationLogs replay data"
    );
}

/// Full daemon restart smoke test: when the durable RocksDB store already
/// contains a ledger tip, `build_node` must read it before P2P starts,
/// advertise it in `version`, and request blocks from `tip + 1`.
#[tokio::test]
async fn build_node_restarts_from_durable_rocksdb_tip_and_resumes_sync_cursor() {
    const DURABLE_TIP: u32 = 1;
    const PEER_HEIGHT: u32 = 3;

    let temp = tempfile::tempdir().expect("temp RocksDB root");
    let storage_path = temp.path().join("chain");
    let settings = Arc::new(ProtocolSettings::default());
    seed_rocksdb_tip(&storage_path, settings.as_ref(), DURABLE_TIP)
        .expect("seed durable RocksDB tip");

    let mut config = NodeConfig::default();
    config.storage.backend = Some("rocksdb".to_string());
    let running = build_node(
        Arc::clone(&settings),
        &config,
        Some(&storage_path),
        None,
        LedgerMode::Local,
        false,
        None,
    )
    .await
    .expect("build node over durable store");

    running
        .network
        .start("127.0.0.1:0".parse().unwrap())
        .await
        .expect("start P2P listener");
    let port = running.network.local_node_info().port();
    assert_ne!(port, 0);

    let mut fake = fake_dial(port).await;
    let node_version = recv_frame(&mut fake).await;
    assert_eq!(node_version.command, MessageCommand::Version);
    let node_version: VersionPayload = decode_payload(&node_version);
    assert!(
        node_version.capabilities.iter().any(|capability| matches!(
            capability,
            NodeCapability::FullNode {
                start_height: DURABLE_TIP
            }
        )),
        "restarted daemon must advertise the durable ledger tip"
    );

    fake.send(fake_peer_version_message(
        settings.network,
        0xfa4e_00d0,
        PEER_HEIGHT,
    ))
    .await
    .expect("send peer version");
    let verack = recv_frame(&mut fake).await;
    assert_eq!(verack.command, MessageCommand::Verack);
    fake.send(verack_message()).await.expect("send verack");

    let request = recv_getblockbyindex(&mut fake).await;
    assert_eq!(
        request.index_start,
        DURABLE_TIP + 1,
        "restart sync cursor resumes just after the durable tip"
    );
    assert_eq!(request.count, (PEER_HEIGHT - DURABLE_TIP) as i16);

    running.network.shutdown().await.expect("shutdown network");
    for handle in running.handles {
        handle.abort();
        let _ = handle.await;
    }
    drop(running.node);
    drop(running.network);
}

/// Operator-facing RPC smoke test: a daemon rebuilt over a durable RocksDB
/// ledger must expose the recovered chain height through JSON-RPC.
///
/// Runs on a multi-thread runtime to match the production daemon
/// (`#[tokio::main]`): the JSON-RPC relay path (`sendrawtransaction` /
/// `submitblock`) uses `block_in_place`, which requires a multi-thread
/// runtime. `getblockcount` itself does not, but the multi-thread flavor
/// keeps this end-to-end smoke test representative of the real daemon.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rpc_getblockcount_reads_restarted_durable_rocksdb_tip() {
    const DURABLE_TIP: u32 = 1;

    let temp = tempfile::tempdir().expect("temp RocksDB root");
    let storage_path = temp.path().join("chain");
    let settings = Arc::new(ProtocolSettings::default());
    seed_rocksdb_tip(&storage_path, settings.as_ref(), DURABLE_TIP)
        .expect("seed durable RocksDB tip");

    let rpc_port = unused_local_rpc_port();
    let mut config = NodeConfig::default();
    config.storage.backend = Some("rocksdb".to_string());
    config.rpc.enabled = true;
    config.rpc.port = Some(rpc_port);
    config.rpc.bind_address = Some("127.0.0.1".to_string());

    let running = build_node(
        Arc::clone(&settings),
        &config,
        Some(&storage_path),
        None,
        LedgerMode::Local,
        false,
        None,
    )
    .await
    .expect("build node over durable store");
    let server = start_rpc_server(&running.node, &config, settings.network, None)
        .expect("start JSON-RPC server");
    assert!(server.read().is_started(), "JSON-RPC server must bind");

    let response = rpc_post_json(
        rpc_port,
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "getblockcount",
            "params": [],
            "id": 1
        }),
    )
    .await;
    assert_eq!(response.get("error"), None);
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"], serde_json::json!(DURABLE_TIP + 1));

    server.write().stop_rpc_server();
    drop(server);
    for handle in running.handles {
        handle.abort();
        let _ = handle.await;
    }
    drop(running.node);
    drop(running.network);
}
