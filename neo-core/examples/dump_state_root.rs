//! Print the locally computed state root for a given height.
//! Usage: dump_state_root <state_root_db_path> <height1> [height2] [height3] ...
#![cfg(feature = "rocksdb")]

use neo_core::persistence::providers::RocksDBStoreProvider;
use neo_core::persistence::{i_store_provider::IStoreProvider, StorageConfig};
use neo_core::state_service::state_store::{
    SnapshotBackedStateStoreBackend, StateServiceSettings, StateStore,
};
use std::path::PathBuf;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: dump_state_root <state_root_db_path> <height1> [height2] ...")?;
    let heights: Vec<u32> = args
        .map(|s| s.parse::<u32>().expect("parse height"))
        .collect();
    if heights.is_empty() {
        return Err("need at least one height".into());
    }

    let provider = RocksDBStoreProvider::new(StorageConfig {
        path: PathBuf::from(&path),
        read_only: true,
        ..StorageConfig::default()
    });
    let store = provider.get_store("")?;
    let backend = Arc::new(SnapshotBackedStateStoreBackend::new(store));
    let state_store = StateStore::new(
        backend,
        StateServiceSettings {
            full_state: true,
            path: path.clone(),
            ..StateServiceSettings::default()
        },
    );

    for h in heights {
        match state_store.get_state_root(h) {
            Some(rec) => println!("{} 0x{}", h, hex::encode(rec.root_hash.as_bytes())),
            None => println!("{} ABSENT", h),
        }
    }
    Ok(())
}
