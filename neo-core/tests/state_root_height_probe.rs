#![cfg(feature = "rocksdb")]

// Probe which mainnet state-root heights are present in `data/mainnet/StateRoot`.
// Companion to the mainnet_block_*_repro tests — those tests silently no-op if
// their predecessor height is missing, which is easy to confuse with "PASS".
// This test reports presence/absence so the harness operator can tell which
// reproducers will exercise their assertions.

use neo_core::persistence::StorageConfig;
use neo_core::persistence::{store_provider::StoreProvider, providers::RocksDBStoreProvider};
use neo_core::state_service::state_store::{
    SnapshotBackedStateStoreBackend, StateServiceSettings, StateStore,
};
use std::path::Path;
use std::sync::Arc;

fn open_state_store() -> StateStore {
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
    StateStore::new(
        backend,
        StateServiceSettings {
            full_state: true,
            path: state_root_path.to_string_lossy().to_string(),
            ..StateServiceSettings::default()
        },
    )
}

#[test]
#[ignore = "diagnostic: lists which mainnet state-root heights are present in local DB"]
fn report_present_heights() {
    let store = open_state_store();

    // Heights covered by existing reproducer tests, plus broad sampling.
    let heights: Vec<u32> = vec![
        0, 1, 100, 1000, 4408, 4409, 4410, 4411, 50_000, 100_000, 150_000, 172_612, 172_613,
        172_614, 200_000, 203_260, 203_261, 203_262, 250_000, 274_156, 274_157, 274_158, 290_000,
        294_368, 294_369, 295_000, 300_000, 350_000, 400_000, 450_000, 500_000,
    ];
    let mut present = Vec::new();
    let mut missing = Vec::new();
    for h in &heights {
        if store.get_state_root(*h).is_some() {
            present.push(*h);
        } else {
            missing.push(*h);
        }
    }
    eprintln!("=== state root presence in data/mainnet/StateRoot ===");
    eprintln!("PRESENT ({}): {:?}", present.len(), present);
    eprintln!("MISSING ({}): {:?}", missing.len(), missing);

    // Find the maximum present height by binary search up to 500k.
    let max_probe = 500_000u32;
    let (mut lo, mut hi) = (0u32, max_probe);
    if store.get_state_root(0).is_some() {
        while lo < hi {
            let mid = lo + (hi - lo).div_ceil(2);
            if store.get_state_root(mid).is_some() {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        eprintln!(
            "highest contiguous-or-present height ≤ {}: {}",
            max_probe, lo
        );
    } else {
        eprintln!("genesis (height 0) not present — DB is empty or corrupt");
    }
}
