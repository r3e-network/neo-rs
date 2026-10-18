#![cfg(feature = "rocksdb")]

//! Contiguous from-genesis execution replay of MainNet blocks.
//!
//! Each block's transactions plus OnPersist/PostPersist are executed against
//! the state trie at the previous state root, then compared with the C#
//! reference root. Default range: blocks 1..=1000; override with
//! `NEO_EMPTY_REPLAY_START` / `NEO_EMPTY_REPLAY_COUNT`.
//!
//! NEO_CHAIN_DB_PATH=data/mainnet-protocol-consistency
//! NEO_STATE_ROOT_DB_PATH=data/mainnet-protocol-consistency/StateRoot
//! cargo test -p neo-core --features rocksdb --test mainnet_contiguous_replay -- --ignored --nocapture

use neo_core::ledger::Block;
use neo_core::persistence::data_cache::{DataCache, DataCacheConfig};
use neo_core::persistence::{
    SeekDirection, StorageConfig, StoreCache, StoreProvider, providers::RocksDBStoreProvider,
};
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::TriggerType;
use neo_core::smart_contract::application_engine::{ApplicationEngine, TEST_MODE_GAS};
use neo_core::smart_contract::native::LedgerTransactionStates;
use neo_core::smart_contract::native::ledger_contract::HashOrIndex;
use neo_core::smart_contract::native::LedgerContract;
use neo_core::smart_contract::native::ledger_contract::PersistedTransactionState;
use neo_core::state_service::state_store::{
    SnapshotBackedStateStoreBackend, StateServiceSettings, StateStore,
};
use neo_core::smart_contract::StorageKey;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn load_reference_roots(path: &Path, start: u32, end: u32) -> HashMap<u32, neo_core::UInt256> {
    let file = File::open(path).expect("open reference roots");
    let mut roots = HashMap::new();
    for line in BufReader::new(file).lines() {
        let line = line.expect("read reference line");
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(&line).expect("parse reference root");
        let Some(height) = value.get("height").and_then(|v| v.as_u64()) else {
            continue;
        };
        let height = height as u32;
        if height < start || height > end {
            continue;
        }
        let root = value
            .get("roothash")
            .and_then(|v| v.as_str())
            .expect("roothash");
        roots.insert(height, neo_core::UInt256::parse(root).expect("parse root"));
    }
    roots
}

fn repo_path(var: &str, default: &str) -> PathBuf {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    std::env::var(var)
        .ok()
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.join(default))
}

fn open_state_store(path: &Path) -> StateStore {
    let provider = RocksDBStoreProvider::new(StorageConfig {
        path: path.to_path_buf(),
        read_only: true,
        ..StorageConfig::default()
    });
    let state_db = provider.get_store("").expect("open state root store");
    let backend = Arc::new(SnapshotBackedStateStoreBackend::new(state_db));
    StateStore::new(
        backend,
        StateServiceSettings {
            full_state: true,
            path: path.to_string_lossy().to_string(),
            ..StateServiceSettings::default()
        },
    )
}

fn execute_empty_block(
    state_store: &StateStore,
    block: Arc<Block>,
    root_prev: neo_core::UInt256,
) -> Option<(neo_core::UInt256, Vec<(Vec<u8>, Option<Vec<u8>>)>)> {
    let trie = Arc::new(Mutex::new(state_store.trie_for_root(root_prev)));
    let store_get = {
        let trie = Arc::clone(&trie);
        Arc::new(move |key: &StorageKey| {
            let mut trie = trie.lock();
            trie.get(&key.to_array())
                .ok()?
                .map(neo_core::smart_contract::StorageItem::from_bytes)
        })
    };
    let store_find = {
        let trie = Arc::clone(&trie);
        Arc::new(
            move |prefix: Option<&StorageKey>, _dir: SeekDirection| {
                let prefix = prefix.map(|key| key.to_array()).unwrap_or_default();
                let mut trie = trie.lock();
                trie.find(&prefix, None)
                    .expect("trie find")
                    .into_iter()
                    .map(|entry| {
                        (
                            StorageKey::from_bytes(&entry.key),
                            neo_core::smart_contract::StorageItem::from_bytes(entry.value),
                        )
                    })
                    .collect()
            },
        )
    };
    let base_cache = Arc::new(DataCache::new_with_config(
        false,
        Some(store_get),
        Some(store_find),
        DataCacheConfig {
            track_reads_in_write_cache: true,
            enable_read_cache: false,
            enable_prefetching: false,
            ..Default::default()
        },
    ));

    let mut on_persist = ApplicationEngine::new_with_shared_block(
        TriggerType::OnPersist,
        None,
        Arc::clone(&base_cache),
        Some(Arc::clone(&block)),
        ProtocolSettings::mainnet(),
        TEST_MODE_GAS,
        None,
    )
    .expect("on persist engine");
    on_persist.native_on_persist().expect("native on persist");
    let seeded_contracts = on_persist.contracts_snapshot();
    let seeded_native_cache = on_persist.native_contract_cache_handle();
    let mut tx_states = on_persist
        .take_state::<LedgerTransactionStates>()
        .unwrap_or_else(|| {
            LedgerTransactionStates::new(
                block
                    .transactions
                    .iter()
                    .map(|tx| PersistedTransactionState::new(tx, block.index()))
                    .collect(),
            )
        });
    drop(on_persist);

    for tx in &block.transactions {
        let tx_store_get = {
            let base = Arc::clone(&base_cache);
            Arc::new(move |key: &StorageKey| base.get(key))
        };
        let tx_store_find = {
            let base = Arc::clone(&base_cache);
            Arc::new(
                move |prefix: Option<&StorageKey>, direction: SeekDirection| {
                    base.find(prefix, direction).collect::<Vec<_>>()
                },
            )
        };
        let tx_snapshot = Arc::new(DataCache::new_with_config(
            false,
            Some(tx_store_get),
            Some(tx_store_find),
            DataCacheConfig {
                track_reads_in_write_cache: false,
                enable_read_cache: false,
                enable_prefetching: false,
                ..Default::default()
            },
        ));
        let mut tx_engine = ApplicationEngine::new_with_preloaded_native(
            TriggerType::Application,
            Some(Arc::new(tx.clone())),
            Arc::clone(&tx_snapshot),
            Some(Arc::clone(&block)),
            ProtocolSettings::mainnet(),
            tx.system_fee(),
            seeded_contracts.clone(),
            Arc::clone(&seeded_native_cache),
            None,
        )
        .expect("tx engine");
        tx_engine.set_state(tx_states);
        tx_engine
            .load_script(tx.script().to_vec(), neo_core::smart_contract::CallFlags::ALL, None)
            .expect("load tx script");
        let _ = tx_engine.execute_allow_fault();
        tx_states = tx_engine
            .take_state::<LedgerTransactionStates>()
            .unwrap_or_else(|| LedgerTransactionStates::new(Vec::new()));
        drop(tx_engine);
        let tracked: Vec<_> = tx_snapshot.tracked_items().into_iter().collect();
        base_cache.merge_tracked_items(&tracked);
    }

    let mut post_persist = ApplicationEngine::new_with_preloaded_native(
        TriggerType::PostPersist,
        None,
        Arc::clone(&base_cache),
        Some(Arc::clone(&block)),
        ProtocolSettings::mainnet(),
        TEST_MODE_GAS,
        seeded_contracts,
        seeded_native_cache,
        None,
    )
    .expect("post persist engine");
    post_persist.set_state(tx_states);
    post_persist
        .native_post_persist()
        .expect("native post persist");
    drop(post_persist);

    let mut tracked: Vec<_> = base_cache.tracked_items().into_iter().collect();
    tracked.sort_by(|a, b| (a.0.id, a.0.key()).cmp(&(b.0.id, b.0.key())));
    drop(base_cache);
    let mut trie_guard = trie.lock();
    let mut touched = Vec::new();
    for (key, trackable) in &tracked {
        let state = format!("{:?}", trackable.state);
        if state == "None" || state == "NotFound" || key.id == -4 {
            continue;
        }
        let key_bytes = key.to_array();
        match state.as_str() {
            "Added" | "Changed" => {
                let value = trackable.item.value_bytes().to_vec();
                trie_guard.put(&key_bytes, &value).expect("trie put");
                touched.push((key_bytes, Some(value)));
            }
            "Deleted" => {
                trie_guard.delete(&key_bytes).expect("trie delete");
                touched.push((key_bytes, None));
            }
            _ => {}
        }
    }
    let root = trie_guard.root_hash()?;
    Some((root, touched))
}

#[test]
#[ignore = "requires local MainNet chain and state-root databases"]
fn contiguous_blocks_replay_to_matching_state_roots() {
    let chain_path = repo_path("NEO_CHAIN_DB_PATH", "data/mainnet-protocol-consistency");
    let state_path = repo_path(
        "NEO_STATE_ROOT_DB_PATH",
        "data/Plugins/StateService/StateRoot",
    );
    if !chain_path.exists() || !state_path.exists() {
        eprintln!("[SKIPPED] missing MainNet chain/state-root DB");
        return;
    }

    let provider = RocksDBStoreProvider::new(StorageConfig {
        path: chain_path,
        read_only: true,
        ..StorageConfig::default()
    });
    let store = provider.get_store("").expect("open chain store");
    let chain_cache = StoreCache::new_from_snapshot(store.snapshot());
    let ledger = LedgerContract::new();
    let state_store = open_state_store(&state_path);

    let start: u32 = std::env::var("NEO_EMPTY_REPLAY_START")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1);
    let end: u32 = std::env::var("NEO_EMPTY_REPLAY_COUNT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1000);
    let reference_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data/reference_stateroots.jsonl");
    let roots = load_reference_roots(&reference_path, start.saturating_sub(1), end);
    let mut current_root = *roots
        .get(&(start.saturating_sub(1)))
        .unwrap_or_else(|| panic!("missing initial reference root"));
    let mut checked = 0u64;
    let mut gaps = 0u64;

    for index in start..=end {
        let block = ledger
            .get_block(&chain_cache, HashOrIndex::Index(index))
            .expect("read block")
            .unwrap_or_else(|| panic!("missing block {index}"));
        let tx_debug: Vec<String> = block
            .transactions
            .iter()
            .map(|tx| hex::encode(tx.script()))
            .collect();
        let (actual, actual_entries) =
            execute_empty_block(&state_store, Arc::new(block), current_root)
                .unwrap_or_else(|| panic!("block {index}: root computation failed"));
        if let Some(expected) = roots.get(&index) {
            if actual != *expected {
                let stored = state_store
                    .get_state_root(index)
                    .map(|root| root.root_hash.to_string())
                    .unwrap_or_else(|| "missing".to_string());
                eprintln!("mismatch block {index}: actual={actual} expected={expected} stored={stored}");
                eprintln!("tx_scripts={tx_debug:?}");
                let mut prev_trie = state_store.trie_for_root(current_root);
                if let Ok(Some(prev_value)) = prev_trie.get(&hex::decode("faffffff0b").unwrap()) {
                    eprintln!("  previous GAS total supply={}", hex::encode(prev_value));
                }
                let mut expected_trie = state_store.trie_for_root(*expected);
                for (key, actual_value) in &actual_entries {
                    let expected_value = expected_trie.get(key).ok().flatten();
                    if expected_value.as_deref() != actual_value.as_deref() {
                        eprintln!(
                            "  key={} expected={:?} actual={:?}",
                            hex::encode(key),
                            expected_value.map(hex::encode),
                            actual_value.as_ref().map(hex::encode)
                        );
                    }
                }
            }
            assert_eq!(actual, *expected, "block {index} state root mismatch");
            checked += 1;
        } else {
            gaps += 1;
            if gaps <= 5 {
                eprintln!("block {index}: reference gap, continuing with computed root");
            }
        }
        current_root = actual;
    }
    eprintln!("replay_range={start}..={end} checked={checked} reference_gaps={gaps}");
}
