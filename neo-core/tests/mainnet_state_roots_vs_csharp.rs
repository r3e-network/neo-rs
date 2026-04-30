#![cfg(feature = "rocksdb")]

// Bulk compat test: for every mainnet height present in the local
// data/mainnet/StateRoot DB up to MAX_CLEAN_HEIGHT that also has a reference
// root in data/reference_stateroots.jsonl, assert the locally-stored state
// root matches the C# canonical root byte-for-byte.
//
// This covers the entire clean range of synced heights, not just the four
// hand-picked reproducer blocks. A single mismatch fails the test.
//
// Why MAX_CLEAN_HEIGHT exists: heights ≥172,613 in the current local DB are
// known stale, produced by a past buggy sync (see mainnet_block_172613_repro
// — current code at that block produces the correct C# root, but the stored
// DB value still reflects the old buggy run). Until the DB is re-synced from
// height 172,613 with current code, the bulk test must cap at 172,612 to
// distinguish real code regressions from known-stale stored data. Set
// `NEO_BULK_ROOT_MAX_HEIGHT` env var to override (e.g. raise after re-sync).
//
// `NEO_BULK_ROOT_DB_PATH` env var overrides the DB path. Useful for testing
// alternate backups, e.g.
//   NEO_BULK_ROOT_DB_PATH=data/mainnet.pre-274157-fix-20260419/StateRoot \
//   NEO_BULK_ROOT_MAX_HEIGHT=257982 \
//   NEO_BULK_ROOT_SKIPLIST=203262 \
//   cargo test ... mainnet_state_roots_vs_csharp -- --ignored --nocapture
//
// `NEO_BULK_ROOT_SKIPLIST` accepts a comma-separated list of heights to skip
// (known bad in the chosen DB but not relevant to current code correctness).

const MAX_CLEAN_HEIGHT_DEFAULT: u32 = 172_612;

use neo_core::persistence::{i_store_provider::IStoreProvider, providers::RocksDBStoreProvider};
use neo_core::persistence::StorageConfig;
use neo_core::state_service::state_store::{
    SnapshotBackedStateStoreBackend, StateServiceSettings, StateStore,
};
use neo_core::UInt256;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::Arc;

fn open_state_store(state_root_path: &Path) -> StateStore {
    let provider = RocksDBStoreProvider::new(StorageConfig {
        path: state_root_path.to_path_buf(),
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
#[ignore = "requires data/mainnet/StateRoot and data/reference_stateroots.jsonl"]
fn local_state_roots_match_csharp_reference() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .to_path_buf();
    // Allow overriding the local state-root DB to test alternate backups
    // (e.g. NEO_BULK_ROOT_DB_PATH=data/mainnet.pre-274157-fix-20260419/StateRoot).
    let state_root_path = match std::env::var("NEO_BULK_ROOT_DB_PATH").ok() {
        Some(p) if !p.is_empty() => Path::new(&p).to_path_buf(),
        _ => repo_root.join("data/mainnet/StateRoot"),
    };
    let reference_path = repo_root.join("data/reference_stateroots.jsonl");

    if !state_root_path.exists() {
        eprintln!(
            "[SKIPPED] {}: data/mainnet/StateRoot directory not present",
            module_path!()
        );
        return;
    }
    if !reference_path.exists() {
        eprintln!(
            "[SKIPPED] {}: data/reference_stateroots.jsonl not present",
            module_path!()
        );
        return;
    }

    let max_height = std::env::var("NEO_BULK_ROOT_MAX_HEIGHT")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(MAX_CLEAN_HEIGHT_DEFAULT);

    let skiplist: std::collections::HashSet<u32> = std::env::var("NEO_BULK_ROOT_SKIPLIST")
        .ok()
        .map(|s| {
            s.split(',')
                .filter_map(|p| p.trim().parse::<u32>().ok())
                .collect()
        })
        .unwrap_or_default();

    let store = open_state_store(&state_root_path);
    let file = File::open(&reference_path).expect("open reference jsonl");
    let reader = BufReader::new(file);

    let mut compared = 0u64;
    let mut skipped_no_local = 0u64;
    let mut skipped_above_cap = 0u64;
    let mut skipped_explicit = 0u64;
    let mut mismatches: Vec<(u32, UInt256, UInt256)> = Vec::new();

    for line in reader.lines() {
        let line = line.expect("read line");
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let height = match v.get("height").and_then(|h| h.as_u64()) {
            Some(h) => h as u32,
            None => continue,
        };
        if height > max_height {
            skipped_above_cap += 1;
            continue;
        }
        if skiplist.contains(&height) {
            skipped_explicit += 1;
            continue;
        }
        let reference_str = match v.get("roothash").and_then(|h| h.as_str()) {
            Some(s) => s,
            None => continue,
        };
        let reference_root = match UInt256::parse(reference_str) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let local_root = match store.get_state_root(height) {
            Some(r) => r.root_hash,
            None => {
                skipped_no_local += 1;
                continue;
            }
        };
        compared += 1;
        if local_root != reference_root {
            mismatches.push((height, local_root, reference_root));
            if mismatches.len() <= 10 {
                eprintln!(
                    "MISMATCH height={} local={} reference={}",
                    height, local_root, reference_root,
                );
            }
        }
    }
    eprintln!(
        "max_height_cap={} compared={} skipped_no_local={} skipped_above_cap={} skipped_explicit={} mismatches={}",
        max_height,
        compared,
        skipped_no_local,
        skipped_above_cap,
        skipped_explicit,
        mismatches.len(),
    );

    assert!(
        compared > 0,
        "no local heights found in reference set — check data layout",
    );
    assert!(
        mismatches.is_empty(),
        "{} state-root mismatch(es) vs C# reference (first up to 10 printed above); first mismatch: height={} local={} reference={}",
        mismatches.len(),
        mismatches[0].0,
        mismatches[0].1,
        mismatches[0].2,
    );
}
