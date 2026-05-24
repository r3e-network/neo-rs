#![cfg(feature = "rocksdb")]

// One-off probe: print the locally-stored state root for a specific height
// from the DB at NEO_REPRO_DB_PATH. Useful to verify whether a backup's
// predecessor for a reproducer test matches the C# canonical value.

use neo_core::persistence::StorageConfig;
use neo_core::persistence::{store_provider::StoreProvider, providers::RocksDBStoreProvider};
use neo_core::state_service::state_store::{
    SnapshotBackedStateStoreBackend, StateServiceSettings, StateStore,
};
use std::path::Path;
use std::sync::Arc;

#[test]
#[ignore = "diagnostic; takes NEO_REPRO_DB_PATH and NEO_PROBE_HEIGHT env vars"]
fn print_height_root() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .to_path_buf();
    let db_path = match std::env::var("NEO_REPRO_DB_PATH").ok() {
        Some(p) if !p.is_empty() => Path::new(&p).to_path_buf(),
        _ => repo_root.join("data/mainnet/StateRoot"),
    };
    let height: u32 = std::env::var("NEO_PROBE_HEIGHT")
        .unwrap_or_else(|_| "0".to_string())
        .parse()
        .expect("NEO_PROBE_HEIGHT must be a u32");

    let provider = RocksDBStoreProvider::new(StorageConfig {
        path: db_path.clone(),
        read_only: true,
        ..StorageConfig::default()
    });
    let state_db = provider.get_store("").expect("open state store");
    let backend = Arc::new(SnapshotBackedStateStoreBackend::new(state_db));
    let store = StateStore::new(
        backend,
        StateServiceSettings {
            full_state: true,
            path: db_path.to_string_lossy().to_string(),
            ..StateServiceSettings::default()
        },
    );
    match store.get_state_root(height) {
        Some(r) => eprintln!(
            "db={} height={} root={}",
            db_path.display(),
            height,
            r.root_hash
        ),
        None => eprintln!("db={} height={} root=ABSENT", db_path.display(), height),
    }
}
