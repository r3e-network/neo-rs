//! Dump local state roots at specific block heights.
#![cfg(feature = "rocksdb")]

use neo_core::persistence::providers::RocksDBStoreProvider;
use neo_core::persistence::{i_store_provider::IStoreProvider, StorageConfig};
use neo_core::state_service::state_store::{
    SnapshotBackedStateStoreBackend, StateServiceSettings, StateStore,
};
use std::path::Path;
use std::sync::Arc;

fn main() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .to_path_buf();
    let state_root_path = repo_root.join("data/mainnet/StateRoot");
    let provider = RocksDBStoreProvider::new(StorageConfig {
        path: state_root_path.clone(),
        read_only: true,
        ..StorageConfig::default()
    });
    let state_db = provider.get_store("").expect("open state root store");
    let backend = Arc::new(SnapshotBackedStateStoreBackend::new(state_db));
    let store = StateStore::new(
        backend,
        StateServiceSettings {
            full_state: true,
            path: state_root_path.to_string_lossy().to_string(),
            ..StateServiceSettings::default()
        },
    );
    let args: Vec<String> = std::env::args().collect();
    let heights: Vec<u32> = if args.len() > 1 {
        args[1..].iter().filter_map(|s| s.parse().ok()).collect()
    } else {
        vec![
            150000, 160000, 165000, 170000, 171000, 172000, 172500, 172600, 172613,
        ]
    };
    for height in heights {
        match store.get_state_root(height) {
            Some(sr) => println!("height={} root={}", height, sr.root_hash),
            None => println!("height={} (no local root)", height),
        }
    }
}
