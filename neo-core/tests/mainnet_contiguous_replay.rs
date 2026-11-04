#![cfg(feature = "rocksdb")]

//! Contiguous from-genesis execution replay of MainNet blocks.
//!
//! Each block's transactions plus OnPersist/PostPersist are executed against
//! the state trie at the previous state root, then compared with the C#
//! reference root. Ledger-contract storage (id `-4`) is excluded from the
//! StateService trie, so those keys are read from the chain store and
//! `Ledger.currentIndex` is overridden with the historical height
//! (`block.index - 1`) rather than the live chain tip.
//! Default range: blocks 1..=1000; override with inclusive
//! `NEO_EMPTY_REPLAY_START` / `NEO_EMPTY_REPLAY_END`. Legacy
//! `NEO_EMPTY_REPLAY_COUNT` remains an alias for the inclusive end height.
//!
//! NEO_CHAIN_DB_PATH=data/mainnet-protocol-consistency
//! NEO_STATE_ROOT_DB_PATH=data/mainnet-protocol-consistency/StateRoot
//! cargo test -p neo-core --features rocksdb --test mainnet_contiguous_replay -- --ignored --nocapture

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use neo_core::ledger::Block;
use neo_core::neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_core::persistence::data_cache::{DataCache, DataCacheConfig};
use neo_core::persistence::{
    SeekDirection, StorageConfig, StoreCache, StoreProvider, providers::RocksDBStoreProvider,
};
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::StorageKey;
use neo_core::smart_contract::TriggerType;
use neo_core::smart_contract::application_engine::{ApplicationEngine, TEST_MODE_GAS};
use neo_core::smart_contract::native::LedgerContract;
use neo_core::smart_contract::native::LedgerTransactionStates;
use neo_core::smart_contract::native::ledger_contract::HashOrIndex;
use neo_core::smart_contract::native::ledger_contract::PersistedTransactionState;
use neo_core::state_service::state_store::{
    SnapshotBackedStateStoreBackend, StateServiceSettings, StateStore, StateStoreBackend,
};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

fn load_reference_roots(
    path: &Path,
    start: u32,
    end: u32,
) -> Result<HashMap<u32, neo_core::UInt256>, String> {
    let file = File::open(path)
        .map_err(|error| format!("open reference roots {}: {error}", path.display()))?;
    let mut roots = HashMap::new();
    for (line_index, line) in BufReader::new(file).lines().enumerate() {
        let line_number = line_index + 1;
        let line = line.map_err(|error| format!("read reference line {line_number}: {error}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(&line)
            .map_err(|error| format!("parse reference line {line_number}: {error}"))?;
        let height = value
            .get("height")
            .and_then(|value| value.as_u64())
            .ok_or_else(|| format!("reference line {line_number}: missing integer height"))?;
        let height = u32::try_from(height)
            .map_err(|_| format!("reference line {line_number}: height exceeds u32"))?;
        if height < start || height > end {
            continue;
        }
        let root = value
            .get("roothash")
            .and_then(|value| value.as_str())
            .ok_or_else(|| format!("reference line {line_number}: missing roothash"))?;
        let root = neo_core::UInt256::parse(root)
            .map_err(|error| format!("reference line {line_number}: invalid root: {error}"))?;
        if roots.insert(height, root).is_some() {
            return Err(format!(
                "reference line {line_number}: duplicate height {height}"
            ));
        }
    }
    Ok(roots)
}

fn require_reference_roots(
    roots: &HashMap<u32, neo_core::UInt256>,
    start: u32,
    end: u32,
) -> Result<(), String> {
    for height in start.saturating_sub(1)..=end {
        if !roots.contains_key(&height) {
            return Err(format!("missing reference root at height {height}"));
        }
    }
    Ok(())
}

fn require_persisted_starting_root(
    state_store: &StateStore,
    height: u32,
    expected: neo_core::UInt256,
) -> Result<neo_core::UInt256, String> {
    let persisted = state_store
        .get_state_root(height)
        .ok_or_else(|| format!("missing persisted starting state root at height {height}"))?;
    if persisted.root_hash != expected {
        return Err(format!(
            "persisted starting state root mismatch at height {height}: stored={} reference={expected}",
            persisted.root_hash
        ));
    }
    Ok(expected)
}

fn env_value(name: &str) -> Result<Option<String>, String> {
    match std::env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(format!("{name} must contain valid UTF-8")),
    }
}

fn parse_height(name: &str, value: String) -> Result<u32, String> {
    value
        .parse::<u32>()
        .map_err(|_| format!("{name} must be an unsigned 32-bit integer, got {value:?}"))
}

fn parse_replay_range(
    start: Option<String>,
    end: Option<String>,
    legacy_count: Option<String>,
) -> Result<(u32, u32), String> {
    if end.is_some() && legacy_count.is_some() {
        return Err(
            "NEO_EMPTY_REPLAY_END and legacy NEO_EMPTY_REPLAY_COUNT are mutually exclusive"
                .to_string(),
        );
    }
    let start = start
        .map(|value| parse_height("NEO_EMPTY_REPLAY_START", value))
        .transpose()?
        .unwrap_or(1);
    let end = end
        .map(|value| parse_height("NEO_EMPTY_REPLAY_END", value))
        .or_else(|| legacy_count.map(|value| parse_height("legacy NEO_EMPTY_REPLAY_COUNT", value)))
        .transpose()?
        .unwrap_or(1000);
    if start == 0 {
        return Err("NEO_EMPTY_REPLAY_START must be at least 1".to_string());
    }
    if end < start {
        return Err(format!(
            "replay range is empty: start {start} exceeds end {end}"
        ));
    }
    Ok((start, end))
}

fn parse_commit_interval(value: Option<String>) -> Result<u32, String> {
    value
        .map(|value| {
            value
                .parse::<u32>()
                .ok()
                .filter(|interval| *interval > 0)
                .ok_or_else(|| {
                    format!("NEO_REPLAY_COMMIT_INTERVAL must be a positive integer, got {value:?}")
                })
        })
        .transpose()
        .map(|interval| interval.unwrap_or(1))
}

fn parse_test_crash_after(value: Option<String>) -> Result<Option<u64>, String> {
    value
        .map(|value| {
            value
                .parse::<u64>()
                .ok()
                .filter(|count| *count > 0)
                .ok_or_else(|| {
                    format!("NEO_REPLAY_TEST_CRASH_AFTER must be a positive integer, got {value:?}")
                })
        })
        .transpose()
}

fn resolve_repo_path(
    repo: &Path,
    override_value: Option<String>,
    default: &str,
) -> Result<(PathBuf, bool), String> {
    match override_value {
        Some(value) if value.is_empty() => Err("path override must not be empty".to_string()),
        Some(value) => {
            let path = PathBuf::from(value);
            Ok((
                if path.is_absolute() {
                    path
                } else {
                    repo.join(path)
                },
                true,
            ))
        }
        None => Ok((repo.join(default), false)),
    }
}

fn repo_path(var: &str, default: &str) -> Result<(PathBuf, bool), String> {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let value = env_value(var)?;
    resolve_repo_path(repo, value, default)
        .map_err(|error| format!("invalid {var} override: {error}"))
}

fn data_directory_available(
    path: &Path,
    label: &str,
    fail_if_missing: bool,
) -> Result<bool, String> {
    if path.is_dir() {
        Ok(true)
    } else if fail_if_missing {
        Err(format!(
            "configured MainNet {label} DB does not exist: {}",
            path.display()
        ))
    } else {
        Ok(false)
    }
}

struct OverlayStateStoreBackend {
    base: Arc<dyn StateStoreBackend>,
    overlay: Arc<dyn StateStoreBackend>,
}

impl StateStoreBackend for OverlayStateStoreBackend {
    fn try_get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.overlay.try_get(key).or_else(|| self.base.try_get(key))
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) {
        self.overlay.put(key, value);
    }

    fn delete(&self, key: &[u8]) {
        self.overlay.delete(key);
    }

    fn commit(&self) -> Result<(), String> {
        self.overlay.commit()
    }

    fn discard_pending(&self) {
        self.overlay.discard_pending();
    }

    fn flush(&self) {
        self.overlay.flush();
    }
}

fn open_state_store(
    path: &Path,
    output_path: Option<&Path>,
) -> (StateStore, Option<Arc<dyn StateStoreBackend>>) {
    let provider = RocksDBStoreProvider::new(StorageConfig {
        path: path.to_path_buf(),
        read_only: true,
        ..StorageConfig::default()
    });
    let state_db = provider.get_store("").expect("open state root store");
    let base: Arc<dyn StateStoreBackend> = Arc::new(SnapshotBackedStateStoreBackend::new(state_db));
    let backend: Arc<dyn StateStoreBackend> = if let Some(output_path) = output_path {
        assert_ne!(output_path, path, "state output must not overwrite history");
        let provider = RocksDBStoreProvider::new(StorageConfig {
            path: output_path.to_path_buf(),
            read_only: false,
            ..StorageConfig::default()
        });
        let output_db = provider.get_store("").expect("open replay state output");
        let output_backend = SnapshotBackedStateStoreBackend::new(output_db);
        // The read-only reference database is a fallback for historical trie
        // nodes, not the durable progress of this independent output. Initialize
        // the overlay's own cursor so a new from-genesis replay does not inherit
        // the reference database's much higher local root index.
        if output_backend
            .try_get(neo_core::state_service::Keys::CURRENT_LOCAL_ROOT_INDEX)
            .is_none()
        {
            output_backend.put(
                neo_core::state_service::Keys::CURRENT_LOCAL_ROOT_INDEX.to_vec(),
                0u32.to_le_bytes().to_vec(),
            );
            output_backend
                .commit()
                .expect("initialize replay state output at genesis");
        }
        Arc::new(OverlayStateStoreBackend {
            base,
            overlay: Arc::new(output_backend),
        })
    } else {
        base
    };
    let write_backend = output_path.map(|_| Arc::clone(&backend));
    (
        StateStore::new(
            backend,
            StateServiceSettings {
                full_state: true,
                path: path.to_string_lossy().to_string(),
                ..StateServiceSettings::default()
            },
        ),
        write_backend,
    )
}

type StateChanges = Vec<(Vec<u8>, Option<Vec<u8>>)>;
type ReplayChanges = (neo_core::UInt256, StateChanges, StateChanges);

fn load_raw_block(path_template: &str, height: u32) -> Block {
    let path = path_template.replace("{height}", &height.to_string());
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .join(path);
    let encoded = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read raw block {}: {error}", path.display()));
    let wire = BASE64
        .decode(encoded.trim())
        .expect("decode raw block Base64");
    let mut reader = MemoryReader::new(&wire);
    let network_block =
        <neo_core::network::p2p::payloads::Block as Serializable>::deserialize(&mut reader)
            .expect("deserialize raw C# block");
    let header = network_block.header;
    let header = neo_core::ledger::BlockHeader::new(
        header.version(),
        *header.prev_hash(),
        *header.merkle_root(),
        header.timestamp(),
        header.nonce(),
        header.index(),
        header.primary_index(),
        *header.next_consensus(),
        vec![header.witness.clone()],
    );
    Block::new(header, network_block.transactions)
}

fn persist_trimmed_block(store: &Arc<dyn neo_core::persistence::Store>, block: &Block) {
    use neo_core::smart_contract::native::trimmed_block::TrimmedBlock;

    let trimmed = TrimmedBlock::try_from_block(block).expect("trim historical block");
    let mut writer = BinaryWriter::new();
    trimmed
        .serialize(&mut writer)
        .expect("serialize historical trimmed block");
    let mut hash_key = vec![9];
    hash_key.extend_from_slice(&block.index().to_be_bytes());
    let mut block_key = vec![5];
    block_key.extend_from_slice(&block.hash().to_bytes());
    let mut snapshot = store.snapshot();
    let snapshot = Arc::get_mut(&mut snapshot).expect("mutable ledger replay snapshot");
    snapshot
        .put(
            StorageKey::new(-4, hash_key).to_array(),
            block.hash().to_bytes().to_vec(),
        )
        .expect("persist historical block index");
    snapshot
        .put(
            StorageKey::new(-4, block_key).to_array(),
            writer.into_bytes(),
        )
        .expect("persist historical trimmed block");
    snapshot
        .try_commit()
        .expect("commit historical trimmed block");
}

fn persist_ledger_changes(
    store: &Arc<dyn neo_core::persistence::Store>,
    changes: &[(Vec<u8>, Option<Vec<u8>>)],
) {
    if changes.is_empty() {
        return;
    }
    let mut snapshot = store.snapshot();
    let snapshot = Arc::get_mut(&mut snapshot).expect("mutable ledger replay snapshot");
    for (key, value) in changes {
        match value {
            Some(value) => snapshot.put(key.clone(), value.clone()),
            None => snapshot.delete(key.clone()),
        }
        .expect("persist replay ledger change");
    }
    snapshot.try_commit().expect("commit replay ledger changes");
}

fn execute_empty_block(
    trie: Arc<
        Mutex<
            neo_core::cryptography::mpt_trie::Trie<
                neo_core::state_service::state_store::StateStoreSnapshot,
            >,
        >,
    >,
    ledger_cache: Arc<StoreCache>,
    block: Arc<Block>,
) -> Option<ReplayChanges> {
    let execution_started = Instant::now();
    let ledger_index = block.index().saturating_sub(1);
    let ledger_current_key = StorageKey::new(-4, vec![12]).to_array();
    let store_get = {
        let trie = Arc::clone(&trie);
        let ledger = Arc::clone(&ledger_cache);
        Arc::new(move |key: &StorageKey| {
            if key.id == -4 {
                if key.to_array() == ledger_current_key {
                    let mut bytes = vec![0u8; 32];
                    bytes.extend_from_slice(&ledger_index.to_le_bytes());
                    return Some(neo_core::smart_contract::StorageItem::from_bytes(bytes));
                }
                return ledger.get(key);
            }
            let mut trie = trie.lock();
            trie.get(&key.to_array())
                .ok()?
                .map(neo_core::smart_contract::StorageItem::from_bytes)
        })
    };
    let store_find = {
        let trie = Arc::clone(&trie);
        let ledger = Arc::clone(&ledger_cache);
        Arc::new(move |prefix: Option<&StorageKey>, _dir: SeekDirection| {
            if let Some(key) = prefix
                && key.id == -4
            {
                return ledger.find(Some(key), SeekDirection::Forward).collect();
            }
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
        })
    };
    let cache_reads =
        block.transactions.len() > 2 && std::env::var_os("NEO_REPLAY_DISABLE_READ_CACHE").is_none();
    let base_cache = Arc::new(DataCache::new_with_config(
        false,
        Some(store_get),
        Some(store_find),
        DataCacheConfig {
            track_reads_in_write_cache: true,
            enable_read_cache: cache_reads,
            enable_prefetching: cache_reads
                && std::env::var_os("NEO_REPLAY_DISABLE_PREFETCH").is_none(),
            ..Default::default()
        },
    ));

    let profile = std::env::var_os("NEO_REPLAY_PROFILE").is_some();
    let phase_started = Instant::now();
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
    if profile {
        eprintln!(
            "profile block={} on_persist={:?}",
            block.index(),
            phase_started.elapsed()
        );
    }
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

    let tx_started = Instant::now();
    let extra_gas = std::env::var("NEO_REPLAY_EXTRA_GAS")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(0);
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
                enable_read_cache: cache_reads,
                enable_prefetching: cache_reads
                    && std::env::var_os("NEO_REPLAY_DISABLE_PREFETCH").is_none(),
                ..Default::default()
            },
        ));
        let mut tx_engine = ApplicationEngine::new_with_preloaded_native(
            TriggerType::Application,
            Some(Arc::new(tx.clone())),
            Arc::clone(&tx_snapshot),
            Some(Arc::clone(&block)),
            ProtocolSettings::mainnet(),
            tx.system_fee().saturating_add(extra_gas),
            seeded_contracts.clone(),
            Arc::clone(&seeded_native_cache),
            None,
        )
        .expect("tx engine");
        tx_engine.set_state(tx_states);
        tx_engine
            .load_script(
                tx.script().to_vec(),
                neo_core::smart_contract::CallFlags::ALL,
                None,
            )
            .expect("load tx script");
        let vm_state = tx_engine.execute_allow_fault();
        if profile {
            eprintln!(
                "profile block={} tx_vm_state={vm_state:?} fault={:?} tracked={} gas_consumed={}",
                block.index(),
                tx_engine.fault_exception(),
                tx_snapshot.tracked_items().len(),
                tx_engine.gas_consumed(),
            );
            for notification in tx_engine.notifications() {
                eprintln!(
                    "profile notification block={} contract={} event={} state={:?}",
                    block.index(),
                    notification.script_hash,
                    notification.event_name,
                    notification.state,
                );
            }
        }
        tx_states = tx_engine
            .take_state::<LedgerTransactionStates>()
            .unwrap_or_else(|| LedgerTransactionStates::new(Vec::new()));
        drop(tx_engine);
        if vm_state == neo_vm::VmState::HALT {
            let tracked: Vec<_> = tx_snapshot.tracked_items().into_iter().collect();
            base_cache.merge_tracked_items(&tracked);
        }
    }

    if profile {
        eprintln!(
            "profile block={} txs={:?}",
            block.index(),
            tx_started.elapsed()
        );
    }
    let post_started = Instant::now();
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
    if profile {
        eprintln!(
            "profile block={} post_persist={:?}",
            block.index(),
            post_started.elapsed()
        );
    }

    let mut tracked: Vec<_> = base_cache.tracked_items().into_iter().collect();
    tracked.sort_by(|a, b| (a.0.id, a.0.key()).cmp(&(b.0.id, b.0.key())));
    drop(base_cache);
    let apply_started = Instant::now();
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
    if profile {
        eprintln!(
            "profile block={} apply={:?}",
            block.index(),
            apply_started.elapsed()
        );
    }
    let ledger_changes = tracked
        .iter()
        .filter(|(key, trackable)| {
            key.id == -4
                && !matches!(
                    format!("{:?}", trackable.state).as_str(),
                    "None" | "NotFound"
                )
        })
        .map(|(key, trackable)| {
            let value = match format!("{:?}", trackable.state).as_str() {
                "Deleted" => None,
                _ => Some(trackable.item.value_bytes().to_vec()),
            };
            (key.to_array(), value)
        })
        .collect();
    if profile {
        eprintln!(
            "profile block={} execute_total={:?}",
            block.index(),
            execution_started.elapsed()
        );
    }
    Some((root, touched, ledger_changes))
}

#[test]
#[ignore = "requires local MainNet chain and state-root databases"]
fn contiguous_blocks_replay_to_matching_state_roots() {
    let start_value = env_value("NEO_EMPTY_REPLAY_START").unwrap_or_else(|error| panic!("{error}"));
    let end_value = env_value("NEO_EMPTY_REPLAY_END").unwrap_or_else(|error| panic!("{error}"));
    let legacy_count_value =
        env_value("NEO_EMPTY_REPLAY_COUNT").unwrap_or_else(|error| panic!("{error}"));
    let has_range_override =
        start_value.is_some() || end_value.is_some() || legacy_count_value.is_some();
    let (start, end) = parse_replay_range(start_value, end_value, legacy_count_value)
        .unwrap_or_else(|error| panic!("invalid replay range: {error}"));
    let commit_interval = parse_commit_interval(
        env_value("NEO_REPLAY_COMMIT_INTERVAL").unwrap_or_else(|error| panic!("{error}")),
    )
    .unwrap_or_else(|error| panic!("invalid replay commit interval: {error}"));
    let test_crash_after = parse_test_crash_after(
        env_value("NEO_REPLAY_TEST_CRASH_AFTER").unwrap_or_else(|error| panic!("{error}")),
    )
    .unwrap_or_else(|error| panic!("invalid replay test crash point: {error}"));
    let profile = std::env::var_os("NEO_REPLAY_PROFILE").is_some();

    let (chain_path, chain_explicit) =
        repo_path("NEO_CHAIN_DB_PATH", "data/mainnet-protocol-consistency")
            .unwrap_or_else(|error| panic!("{error}"));
    let (state_path, state_explicit) = repo_path(
        "NEO_STATE_ROOT_DB_PATH",
        "data/mainnet-protocol-consistency/StateRoot",
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let has_explicit_override = has_range_override || chain_explicit || state_explicit;
    let chain_available = data_directory_available(&chain_path, "chain", has_explicit_override)
        .unwrap_or_else(|error| panic!("{error}"));
    let state_available =
        data_directory_available(&state_path, "state-root", has_explicit_override)
            .unwrap_or_else(|error| panic!("{error}"));
    if !chain_available || !state_available {
        eprintln!("[SKIPPED] missing default MainNet chain/state-root DB");
        return;
    }

    let provider = RocksDBStoreProvider::new(StorageConfig {
        path: chain_path,
        read_only: true,
        ..StorageConfig::default()
    });
    let store = provider.get_store("").expect("open chain store");
    let chain_cache = Arc::new(StoreCache::new_from_snapshot(store.snapshot()));
    let ledger = LedgerContract::new();
    let state_output_path = env_value("NEO_REPLAY_STATE_OUTPUT_PATH")
        .unwrap_or_else(|error| panic!("{error}"))
        .map(|value| {
            assert!(!value.is_empty(), "NEO_REPLAY_STATE_OUTPUT_PATH is empty");
            let path = PathBuf::from(value);
            if path.is_absolute() {
                path
            } else {
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .expect("repo root")
                    .join(path)
            }
        });
    let (state_store, state_write_backend) =
        open_state_store(&state_path, state_output_path.as_deref());

    let reference_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data/reference_stateroots.jsonl");
    let roots = load_reference_roots(&reference_path, start.saturating_sub(1), end)
        .unwrap_or_else(|error| panic!("{error}"));
    require_reference_roots(&roots, start, end).unwrap_or_else(|error| panic!("{error}"));
    let starting_height = start.saturating_sub(1);
    let mut current_root =
        require_persisted_starting_root(&state_store, starting_height, roots[&starting_height])
            .unwrap_or_else(|error| panic!("{error}"));
    let isolated_benchmark = state_output_path
        .as_deref()
        .is_some_and(|path| path.join("BENCHMARK_ONLY").is_file());
    if state_write_backend.is_some() && !isolated_benchmark {
        let durable_height = state_store
            .local_root_index()
            .unwrap_or_else(|| panic!("missing durable replay local root index"));
        assert_eq!(
            durable_height,
            starting_height,
            "writable replay must resume at durable height + 1; rerun with --start {}",
            durable_height.saturating_add(1)
        );
    }
    // Trie::commit clears its mutable node cache, so the same in-memory trie can
    // continue across blocks. Durable backend commits are independently batched.
    let trie = Arc::new(Mutex::new(state_store.trie_for_root(current_root)));
    let mut checked = 0u64;
    let mut pending_ledger_changes = Vec::new();
    let raw_block_path =
        env_value("NEO_REPLAY_BLOCK_PATH").unwrap_or_else(|error| panic!("{error}"));
    assert!(
        raw_block_path
            .as_deref()
            .is_none_or(|path| start == end || path.contains("{height}")),
        "multi-block NEO_REPLAY_BLOCK_PATH must contain the {{height}} placeholder"
    );
    let (ledger_path, _) = repo_path("NEO_REPLAY_LEDGER_PATH", "data/replay-ledger-mainnet")
        .unwrap_or_else(|error| panic!("{error}"));
    let ledger_provider = RocksDBStoreProvider::new(StorageConfig {
        path: ledger_path,
        read_only: false,
        ..StorageConfig::default()
    });
    let ledger_store = ledger_provider
        .get_store("")
        .expect("open replay ledger store");
    let ledger_cache = Arc::new(StoreCache::new_from_store(Arc::clone(&ledger_store), true));
    if let Some(path) = raw_block_path.as_deref() {
        for height in start.saturating_sub(2)..start {
            if ledger
                .get_block_hash_by_index(ledger_cache.as_ref(), height)
                .expect("read replay block index")
                .is_none()
            {
                persist_trimmed_block(&ledger_store, &load_raw_block(path, height));
            }
        }
    }

    let replay_loop_started = Instant::now();
    for index in start..=end {
        let block_started = Instant::now();
        let block = if let Some(path) = raw_block_path.as_deref() {
            load_raw_block(path, index)
        } else {
            ledger
                .get_block(chain_cache.as_ref(), HashOrIndex::Index(index))
                .expect("read block")
                .unwrap_or_else(|| panic!("missing block {index}"))
        };
        assert_eq!(block.index(), index, "raw block height mismatch");
        let block = Arc::new(block);
        let (actual, actual_entries, ledger_changes) = execute_empty_block(
            Arc::clone(&trie),
            Arc::clone(&ledger_cache),
            Arc::clone(&block),
        )
        .unwrap_or_else(|| panic!("block {index}: root computation failed"));
        if let Some(expected) = roots.get(&index) {
            if actual != *expected {
                let tx_debug: Vec<String> = block
                    .transactions
                    .iter()
                    .map(|tx| {
                        format!(
                            "script={} attrs={:?}",
                            hex::encode(tx.script()),
                            tx.attributes()
                        )
                    })
                    .collect();
                let stored = state_store
                    .get_state_root(index)
                    .map(|root| root.root_hash.to_string())
                    .unwrap_or_else(|| "missing".to_string());
                eprintln!(
                    "mismatch block {index}: actual={actual} expected={expected} stored={stored}"
                );
                eprintln!("tx_scripts={tx_debug:?}");
                for height in index.saturating_sub(2)..=index.saturating_add(2) {
                    if let Some(root) = state_store.get_state_root(height) {
                        let mut trie = state_store.trie_for_root(root.root_hash);
                        if let Ok(Some(value)) = trie.get(&hex::decode("faffffff0b").unwrap()) {
                            eprintln!("  height {height} GAS supply={}", hex::encode(value));
                        }
                    }
                }
                let mut gas_trie = state_store.trie_for_root(current_root);
                let gas_prefix = neo_core::smart_contract::StorageKey::create(-5, 29).to_array();
                if let Ok(records) = gas_trie.find(&gas_prefix, None) {
                    for entry in records {
                        eprintln!(
                            "  gas record key={} value={}",
                            hex::encode(entry.key),
                            hex::encode(entry.value)
                        );
                    }
                }
                let mut prev_trie = state_store.trie_for_root(current_root);
                let mut expected_trie = state_store.trie_for_root(*expected);
                let account_prefix =
                    neo_core::smart_contract::StorageKey::create(-6, 20).to_array();
                if let Ok(accounts) = expected_trie.find(&account_prefix, None) {
                    for entry in accounts {
                        let previous = prev_trie.get(&entry.key).ok().flatten();
                        if previous.as_deref() != Some(entry.value.as_slice()) {
                            eprintln!(
                                "  expected account change key={} prev={:?} new={}",
                                hex::encode(&entry.key),
                                previous.map(hex::encode),
                                hex::encode(&entry.value)
                            );
                        }
                    }
                }
                for (key, value) in &actual_entries {
                    if key.starts_with(&account_prefix) {
                        eprintln!(
                            "  actual account change key={} new={:?}",
                            hex::encode(key),
                            value.as_ref().map(hex::encode)
                        );
                    }
                }
                if let Ok(Some(prev_value)) = prev_trie.get(&hex::decode("faffffff0b").unwrap()) {
                    eprintln!("  previous GAS total supply={}", hex::encode(prev_value));
                }
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
            panic!("missing reference root at height {index}");
        }
        if let Some(backend) = state_write_backend.as_ref() {
            pending_ledger_changes.extend(ledger_changes);
            let state_root = neo_core::state_service::StateRoot::new_current(index, actual);
            let mut writer = BinaryWriter::new();
            state_root
                .serialize(&mut writer)
                .expect("serialize state root");
            backend.put(
                neo_core::state_service::Keys::state_root(index),
                writer.into_bytes(),
            );
            backend.put(
                neo_core::state_service::Keys::CURRENT_LOCAL_ROOT_INDEX.to_vec(),
                index.to_le_bytes().to_vec(),
            );
            if checked % u64::from(commit_interval) == 0 || index == end {
                let commit_started = Instant::now();
                let mut trie_guard = trie.lock();
                let flush_started = Instant::now();
                trie_guard.flush_dirty().expect("flush replay trie nodes");
                if profile {
                    eprintln!(
                        "profile block={index} trie_flush={:?}",
                        flush_started.elapsed()
                    );
                }
                let cache_commit_started = Instant::now();
                trie_guard.commit().expect("stage replay trie nodes");
                if profile {
                    eprintln!(
                        "profile block={index} trie_cache_commit={:?}",
                        cache_commit_started.elapsed()
                    );
                    eprintln!(
                        "profile block={index} trie_commit={:?}",
                        commit_started.elapsed()
                    );
                }
                drop(trie_guard);
                let durable_started = Instant::now();
                let ledger_started = Instant::now();
                persist_ledger_changes(&ledger_store, &pending_ledger_changes);
                if profile {
                    eprintln!(
                        "profile block={index} ledger_commit={:?}",
                        ledger_started.elapsed()
                    );
                }
                pending_ledger_changes.clear();
                let backend_started = Instant::now();
                backend.commit().expect("persist replay state output");
                if profile {
                    eprintln!(
                        "profile block={index} state_backend_commit={:?}",
                        backend_started.elapsed()
                    );
                    eprintln!(
                        "profile block={index} durable_commit={:?} commit_interval={commit_interval}",
                        durable_started.elapsed()
                    );
                }
            }
            let readback_started = Instant::now();
            let persisted = state_store
                .get_state_root(index)
                .unwrap_or_else(|| panic!("missing replay output root at height {index}"));
            assert_eq!(persisted.root_hash, actual, "replay output root mismatch");
            if profile {
                eprintln!(
                    "profile block={index} state_root_readback={:?}",
                    readback_started.elapsed()
                );
            }
        }
        current_root = actual;
        if profile {
            eprintln!(
                "profile block={index} block_total={:?}",
                block_started.elapsed()
            );
        }
        if test_crash_after == Some(checked) {
            eprintln!(
                "replay_test_crash_after={checked} last_height={index} commit_interval={commit_interval}"
            );
            std::process::exit(70);
        }
    }
    if let Some(backend) = state_write_backend.as_ref() {
        backend.flush();
    }
    if profile {
        eprintln!(
            "profile replay_loop_total={:?} checked={checked} commit_interval={commit_interval}",
            replay_loop_started.elapsed()
        );
    }
    let expected_checks = u64::from(end) - u64::from(start) + 1;
    assert_eq!(
        checked, expected_checks,
        "replay completed without comparing every requested state root"
    );
    eprintln!("replay_range={start}..={end} checked={checked} reference_gaps=0");
}

#[test]
#[ignore = "prints the durable replay height for the configured local state output"]
fn print_replay_state_height() {
    let state_path = repo_path(
        "NEO_STATE_ROOT_DB_PATH",
        "data/mainnet-protocol-consistency/StateRoot",
    )
    .unwrap_or_else(|error| panic!("{error}"))
    .0;
    let output_path = repo_path("NEO_REPLAY_STATE_OUTPUT_PATH", "data/replay-state-mainnet")
        .unwrap_or_else(|error| panic!("{error}"))
        .0;
    let (state_store, _) = open_state_store(&state_path, Some(&output_path));
    let height = state_store
        .local_root_index()
        .expect("missing durable replay local root index");
    println!("replay_local_root_index={height}");
}

#[test]
#[ignore = "inspects a historical block in the configured local MainNet chain store"]
fn print_historical_ledger_block() {
    let chain_path = repo_path("NEO_CHAIN_DB_PATH", "data/mainnet-protocol-consistency")
        .unwrap_or_else(|error| panic!("{error}"))
        .0;
    let index = env_value("NEO_REPLAY_LEDGER_BLOCK")
        .unwrap_or_else(|error| panic!("{error}"))
        .map(|value| parse_height("NEO_REPLAY_LEDGER_BLOCK", value))
        .transpose()
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!("NEO_REPLAY_LEDGER_BLOCK is required"));
    let provider = RocksDBStoreProvider::new(StorageConfig {
        path: chain_path,
        read_only: true,
        ..StorageConfig::default()
    });
    let store = provider.get_store("").expect("open chain store");
    let chain_cache = StoreCache::new_from_snapshot(store.snapshot());
    let ledger = LedgerContract::new();
    let hash = ledger
        .get_block_hash_by_index(&chain_cache, index)
        .expect("read block hash");
    let block = ledger
        .get_block(&chain_cache, HashOrIndex::Index(index))
        .expect("read block");
    println!(
        "index={index} hash={hash:?} block_present={}",
        block.is_some()
    );
    if let Some(block) = block {
        println!(
            "block_index={} transactions={}",
            block.index(),
            block.transactions.len()
        );
    }
}

#[cfg(test)]
mod configuration_tests {
    use super::*;

    const ZERO_ROOT: &str = "0x0000000000000000000000000000000000000000000000000000000000000000";

    #[test]
    fn replay_range_defaults_and_legacy_count_remain_supported() {
        assert_eq!(parse_replay_range(None, None, None), Ok((1, 1000)));
        assert_eq!(
            parse_replay_range(Some("200".to_string()), None, Some("250".to_string())),
            Ok((200, 250))
        );
        assert_eq!(
            parse_replay_range(Some("200".to_string()), Some("250".to_string()), None),
            Ok((200, 250))
        );
    }

    #[test]
    fn durable_commit_interval_is_positive_and_defaults_to_per_block() {
        assert_eq!(parse_commit_interval(None), Ok(1));
        assert_eq!(parse_commit_interval(Some("64".to_string())), Ok(64));
        for malformed in ["", "0", "abc", "-1", "1.5", "4294967296"] {
            assert!(parse_commit_interval(Some(malformed.to_string())).is_err());
        }
    }

    #[test]
    fn test_crash_point_is_positive_and_opt_in() {
        assert_eq!(parse_test_crash_after(None), Ok(None));
        assert_eq!(
            parse_test_crash_after(Some("700".to_string())),
            Ok(Some(700))
        );
        for malformed in ["", "0", "abc", "-1", "1.5"] {
            assert!(parse_test_crash_after(Some(malformed.to_string())).is_err());
        }
    }

    #[test]
    fn malformed_or_empty_replay_ranges_are_rejected() {
        for malformed in ["", "abc", "-1", "1.5", "4294967296"] {
            assert!(parse_replay_range(Some(malformed.to_string()), None, None).is_err());
            assert!(parse_replay_range(None, Some(malformed.to_string()), None).is_err());
            assert!(parse_replay_range(None, None, Some(malformed.to_string())).is_err());
        }
        assert!(parse_replay_range(Some("0".to_string()), None, None).is_err());
        assert!(parse_replay_range(Some("11".to_string()), Some("10".to_string()), None).is_err());
        assert!(parse_replay_range(None, Some("10".to_string()), Some("10".to_string())).is_err());
    }

    #[test]
    fn relative_data_overrides_resolve_against_repo_and_missing_explicit_paths_fail() {
        let repo =
            std::env::temp_dir().join(format!("neo-rs-replay-missing-repo-{}", std::process::id()));
        let (resolved, explicit) = resolve_repo_path(
            &repo,
            Some("data/missing-chain".to_string()),
            "data/default-chain",
        )
        .expect("resolve relative override");
        assert!(explicit);
        assert_eq!(resolved, repo.join("data/missing-chain"));
        let error = data_directory_available(&resolved, "chain", true).unwrap_err();
        assert!(error.contains("does not exist"));

        let (default_path, explicit) =
            resolve_repo_path(&repo, None, "data/default-chain").expect("resolve default");
        assert!(!explicit);
        assert!(!data_directory_available(&default_path, "chain", false).unwrap());
    }

    #[test]
    fn empty_path_override_is_rejected() {
        assert!(resolve_repo_path(Path::new("repo"), Some(String::new()), "data").is_err());
    }

    #[test]
    fn truncated_and_incomplete_reference_data_are_rejected() {
        let path = std::env::temp_dir().join(format!(
            "neo-rs-replay-truncated-roots-{}.jsonl",
            std::process::id()
        ));
        std::fs::write(&path, "{\"height\": 1, \"roothash\":").expect("write truncated fixture");
        let error = load_reference_roots(&path, 0, 2).unwrap_err();
        assert!(error.contains("parse reference line 1"));

        std::fs::write(
            &path,
            format!(
                "{{\"height\":0,\"roothash\":\"{ZERO_ROOT}\"}}\n\
                 {{\"height\":2,\"roothash\":\"{ZERO_ROOT}\"}}\n"
            ),
        )
        .expect("write incomplete fixture");
        let roots = load_reference_roots(&path, 0, 2).expect("load incomplete fixture");
        let error = require_reference_roots(&roots, 1, 2).unwrap_err();
        assert_eq!(error, "missing reference root at height 1");

        std::fs::remove_file(path).expect("remove reference fixture");
    }

    #[test]
    fn missing_persisted_starting_root_is_rejected() {
        let store = StateStore::new_in_memory();
        let expected = neo_core::UInt256::from([1u8; 32]);

        let error = require_persisted_starting_root(&store, 0, expected).unwrap_err();

        assert_eq!(error, "missing persisted starting state root at height 0");
    }

    #[test]
    fn mismatched_persisted_starting_root_is_rejected() {
        use neo_core::state_service::StateRoot;

        let store = StateStore::new_in_memory();
        let stored = neo_core::UInt256::from([1u8; 32]);
        let expected = neo_core::UInt256::from([2u8; 32]);
        let mut snapshot = store.snapshot();
        snapshot
            .add_local_state_root(&StateRoot::new_current(0, stored))
            .expect("add starting root");
        snapshot.commit().expect("commit starting root");

        let error = require_persisted_starting_root(&store, 0, expected).unwrap_err();

        assert_eq!(
            error,
            format!(
                "persisted starting state root mismatch at height 0: stored={stored} reference={expected}"
            )
        );
    }

    #[test]
    fn complete_reference_range_is_accepted() {
        let roots = HashMap::from([
            (0, neo_core::UInt256::zero()),
            (1, neo_core::UInt256::zero()),
            (2, neo_core::UInt256::zero()),
        ]);
        assert_eq!(require_reference_roots(&roots, 1, 2), Ok(()));
    }
}
