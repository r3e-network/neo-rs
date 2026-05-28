#![cfg(feature = "rocksdb")]

// Reproducer for block 1,465,790 — Bug #13. State root divergence first appears
// at this block (last byte-exact match was h=1,465,789).
//
// 2 txs:
// - tx[0]: 0x522e338ce3955f00fdcdd551b0dfa2ffe3ccdcac3bcbca285dea84108fc46f8b
//   - sender 0xb8a020fce295c9e36ab7ec3502c9ebbabf2d8878 (NWuHQdxabXPdC6vVwJhxjYELDQPqc1d4TG)
//   - sysfee=1852365, netfee=123552, validuntilblock=1471548
//   - calls `transfer` on contract 0xf015 1f5281 75 5827d6...
// - tx[1]: 0x744424c7194cf467c9677c04b56389e55bf28e3e48f44de69559c85ad4daaf7b
//   - sender 0x3f699a30c273a1b39e1346dd63dfafa384977f94 (NZTA3PJBp9zYyj32Cozheuxqo7S1yqC9Vj)
//   - sysfee=26929302, netfee=127062, validuntilblock=1465820
//   - LRB.transfer(from, Aviary, amount, ["ACTION_SWAP", LUSD, 0])
//   - LRB=0x8c07b4c9f5bc170a3922eac4f5bb7ef17b0acc8b  (LyrebirdToken)
//   - LUSD=0xa8c51aa0c177187aeed3db88bdfa908ccbc9b1a5 (LyrebirdUSDToken)
//   - Aviary=0x4768c475e4c8465f2edf97f265c85950dfebc787 (LyrebirdAviary AMM)
//
// C# expected state root @ 1,465,790: 0xd84d614a7b4e5ac08ec3d834cc680831f05fc561ea5e9b5892471fb51e52248e
// Rust computed (divergent):          0x7e56a202c20f1089dc86326d6e39b7a0928bcbb308b6d94066f3ed819b3d66c6
// put_count=13 del_count=1 from block

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use neo_core::ledger::{Block, BlockHeader};
use neo_core::network::p2p::payloads::{
    signer::Signer, transaction::Transaction, witness::Witness,
};
use neo_core::persistence::data_cache::{DataCache, DataCacheConfig};
use neo_core::persistence::{store_provider::StoreProvider, providers::RocksDBStoreProvider};
use neo_core::persistence::{SeekDirection, StorageConfig};
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::CallFlags;
use neo_core::smart_contract::native::ledger_contract::PersistedTransactionState;
use neo_core::smart_contract::native::LedgerTransactionStates;
use neo_core::smart_contract::TriggerType;
use neo_core::state_service::state_store::{
    SnapshotBackedStateStoreBackend, StateServiceSettings, StateStore,
};
use neo_core::{UInt160, UInt256};
use parking_lot::Mutex;
use std::path::Path;
use std::sync::Arc;

fn u160(hex: &str) -> UInt160 {
    UInt160::parse(hex).expect("valid UInt160")
}

fn u256(hex: &str) -> UInt256 {
    UInt256::parse(hex).expect("valid UInt256")
}

fn u160_from_address(addr: &str) -> UInt160 {
    UInt160::from_address(addr).expect("valid address")
}

fn witness(invocation_b64: &str, verification_b64: &str) -> Witness {
    Witness::new_with_scripts(
        BASE64.decode(invocation_b64).expect("base64 invocation"),
        BASE64
            .decode(verification_b64)
            .expect("base64 verification"),
    )
}

fn open_state_store() -> StateStore {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .to_path_buf();
    let state_root_path = match std::env::var("NEO_REPRO_DB_PATH").ok() {
        Some(p) if !p.is_empty() => Path::new(&p).to_path_buf(),
        _ => repo_root.join("data/Plugins/mainnet/StateRoot"),
    };
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

const TX0_SCRIPT: &str = include_str!("mainnet_block_1465790_data/tx0_script.b64");
const TX0_INV: &str = include_str!("mainnet_block_1465790_data/tx0_inv0.b64");
const TX0_VER: &str = include_str!("mainnet_block_1465790_data/tx0_ver0.b64");
const TX1_SCRIPT: &str = include_str!("mainnet_block_1465790_data/tx1_script.b64");
const TX1_INV: &str = include_str!("mainnet_block_1465790_data/tx1_inv0.b64");
const TX1_VER: &str = include_str!("mainnet_block_1465790_data/tx1_ver0.b64");

fn build_tx0() -> Transaction {
    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(1_950_845_805);
    tx.set_system_fee(1_852_365);
    tx.set_network_fee(123_552);
    tx.set_valid_until_block(1_471_548);
    let signer = Signer::new(
        u160("0xb8a020fce295c9e36ab7ec3502c9ebbabf2d8878"),
        neo_core::WitnessScope::CALLED_BY_ENTRY,
    );
    tx.set_signers(vec![signer]);
    tx.set_attributes(Vec::new());
    tx.set_script(BASE64.decode(TX0_SCRIPT.trim()).expect("tx0 script"));
    tx.set_witnesses(vec![witness(TX0_INV.trim(), TX0_VER.trim())]);
    tx
}

fn build_tx1() -> Transaction {
    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(512_347_098);
    tx.set_system_fee(26_929_302);
    tx.set_network_fee(127_062);
    tx.set_valid_until_block(1_465_820);
    let signer = Signer::new(
        u160("0x3f699a30c273a1b39e1346dd63dfafa384977f94"),
        neo_core::WitnessScope::CALLED_BY_ENTRY,
    );
    tx.set_signers(vec![signer]);
    tx.set_attributes(Vec::new());
    tx.set_script(BASE64.decode(TX1_SCRIPT.trim()).expect("tx1 script"));
    tx.set_witnesses(vec![witness(TX1_INV.trim(), TX1_VER.trim())]);
    tx
}

fn run_tx_engine(
    tx: &Transaction,
    block: Arc<Block>,
    base_cache: Arc<DataCache>,
    tx_states: LedgerTransactionStates,
    seeded_contracts: std::collections::HashMap<UInt160, neo_core::smart_contract::ContractState>,
    seeded_native_cache: Arc<Mutex<neo_core::smart_contract::native::NativeContractsCache>>,
    label: &str,
) -> LedgerTransactionStates {
    let tx_store_get = {
        let base = Arc::clone(&base_cache);
        Arc::new(move |key: &neo_core::smart_contract::StorageKey| base.get(key))
    };
    let tx_store_find = {
        let base = Arc::clone(&base_cache);
        Arc::new(
            move |prefix: Option<&neo_core::smart_contract::StorageKey>, d: SeekDirection| {
                base.find(prefix, d).collect::<Vec<_>>()
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
        seeded_contracts,
        seeded_native_cache,
        None,
    )
    .expect("tx engine");
    tx_engine.set_state(tx_states);
    tx_engine
        .load_script(tx.script().to_vec(), CallFlags::ALL, None)
        .expect("load");

    let vm_state = tx_engine.execute_allow_fault();
    let gas = tx_engine.gas_consumed();
    let exception = tx_engine.fault_exception();
    let notifs = tx_engine.notifications();
    eprintln!("\n=== {label} result ===\nvm_state={vm_state:?} gas={gas} exception={exception:?}");
    eprintln!("notifications ({}):", notifs.len());
    for (i, n) in notifs.iter().take(30).enumerate() {
        eprintln!(
            "  [{}] contract={} event={}",
            i, n.script_hash, n.event_name
        );
    }
    if notifs.len() > 30 {
        eprintln!("  ... +{} more", notifs.len() - 30);
    }

    let final_states = tx_engine
        .take_state::<LedgerTransactionStates>()
        .unwrap_or_else(|| LedgerTransactionStates::new(Vec::new()));

    // Only merge tracked items on HALT (matches C# behavior: faulted tx
    // discards application changes, keeps only fee burn from on_persist).
    if vm_state == neo_vm_rs::VmState::HALT {
        let tracked: Vec<_> = tx_snapshot.tracked_items().into_iter().collect();
        eprintln!("  merging {} tracked items from {label}", tracked.len());
        base_cache.merge_tracked_items(&tracked);
    } else {
        eprintln!("  {label} FAULTED — not merging tx changes (matches C# discard semantics)");
    }

    final_states
}

#[test]
#[ignore = "requires local mainnet full-state data synced past height 1,465,789"]
fn replay_block_1465790_assert_csharp_root() {
    let state_store = open_state_store();
    let Some(root_prev) = state_store.get_state_root(1_465_789).map(|r| r.root_hash) else {
        eprintln!("[SKIPPED] state root 1,465,789 not present in StateRoot DB.");
        return;
    };
    eprintln!("loaded prior state root @1,465,789 = {}", root_prev);
    let trie = Arc::new(Mutex::new(state_store.trie_for_root(root_prev)));

    let store_get = {
        let trie = Arc::clone(&trie);
        Arc::new(move |key: &neo_core::smart_contract::StorageKey| {
            let mut trie = trie.lock();
            trie.get(&key.to_array())
                .expect("trie get")
                .map(neo_core::smart_contract::StorageItem::from_bytes)
        })
    };
    let store_find = {
        let trie = Arc::clone(&trie);
        Arc::new(
            move |prefix: Option<&neo_core::smart_contract::StorageKey>, _dir: SeekDirection| {
                let prefix_bytes = prefix.map(|k| k.to_array().to_owned()).unwrap_or_default();
                let mut trie = trie.lock();
                trie.find(&prefix_bytes, None)
                    .expect("trie find")
                    .into_iter()
                    .map(|entry| {
                        (
                            neo_core::smart_contract::StorageKey::from_bytes(&entry.key),
                            neo_core::smart_contract::StorageItem::from_bytes(entry.value),
                        )
                    })
                    .collect::<Vec<_>>()
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

    let tx0 = build_tx0();
    let tx1 = build_tx1();

    let header = BlockHeader::new(
        0,
        u256("0xbe276ec4e25894636137aef36f96e34f440c2f4ba002c950494e9d3f3c45b3b9"),
        u256("0xfe06f88d9cc1d17dc8c654543cc6b1c1343e615ba6bf23fea40e84f8aa4ee5e2"),
        1_651_225_843_225,
        6_843_653_343_259_205_602, // 0x5EF98A74DAE11FE2
        1_465_790,
        4,
        u160_from_address("NSiVJYZej4XsxG5CUpdwn7VRQk8iiiDMPM"),
        vec![],
    );
    let block = Arc::new(Block::new(header, vec![tx0.clone(), tx1.clone()]));

    // Seed LedgerContract.current_block with prev block's hash+index.
    // State trie excludes LedgerContract (id=-4), so on_persist's read of
    // current_index would return 0 without this seed.
    {
        let ledger_id: i32 = -4;
        let prefix_current_block: u8 = 12;
        let prev_block_hash =
            u256("0xbe276ec4e25894636137aef36f96e34f440c2f4ba002c950494e9d3f3c45b3b9");
        let prev_block_index: u32 = 1_465_789;
        let mut serialized = Vec::with_capacity(36);
        serialized.extend_from_slice(&prev_block_hash.to_bytes());
        serialized.extend_from_slice(&prev_block_index.to_le_bytes());
        let key = neo_core::smart_contract::StorageKey::new(ledger_id, vec![prefix_current_block]);
        base_cache.add(
            key,
            neo_core::smart_contract::StorageItem::from_bytes(serialized),
        );
    }

    let mut on_persist_engine = ApplicationEngine::new_with_shared_block(
        TriggerType::OnPersist,
        None,
        Arc::clone(&base_cache),
        Some(Arc::clone(&block)),
        ProtocolSettings::mainnet(),
        neo_core::smart_contract::application_engine::TEST_MODE_GAS,
        None,
    )
    .expect("on persist engine");
    on_persist_engine.native_on_persist().expect("on persist");
    let seeded_contracts = on_persist_engine.contracts_snapshot();
    let seeded_native_cache = on_persist_engine.native_contract_cache_handle();
    let tx_states = on_persist_engine
        .take_state::<LedgerTransactionStates>()
        .unwrap_or_else(|| {
            LedgerTransactionStates::new(vec![
                PersistedTransactionState::new(&tx0, block.index()),
                PersistedTransactionState::new(&tx1, block.index()),
            ])
        });
    drop(on_persist_engine);

    let tx_states = run_tx_engine(
        &tx0,
        Arc::clone(&block),
        Arc::clone(&base_cache),
        tx_states,
        seeded_contracts.clone(),
        Arc::clone(&seeded_native_cache),
        "tx[0]",
    );

    let tx_states = run_tx_engine(
        &tx1,
        Arc::clone(&block),
        Arc::clone(&base_cache),
        tx_states,
        seeded_contracts.clone(),
        Arc::clone(&seeded_native_cache),
        "tx[1]",
    );

    let mut post_persist_engine = ApplicationEngine::new_with_preloaded_native(
        TriggerType::PostPersist,
        None,
        Arc::clone(&base_cache),
        Some(Arc::clone(&block)),
        ProtocolSettings::mainnet(),
        neo_core::smart_contract::application_engine::TEST_MODE_GAS,
        seeded_contracts,
        seeded_native_cache,
        None,
    )
    .expect("post persist engine");
    post_persist_engine.set_state(tx_states);
    post_persist_engine
        .native_post_persist()
        .expect("post persist");
    drop(post_persist_engine);

    eprintln!("\n=== ALL writes (non-None state) ===");
    let mut all: Vec<_> = base_cache.tracked_items().into_iter().collect();
    all.sort_by(|a, b| (a.0.id, a.0.key()).cmp(&(b.0.id, b.0.key())));
    let mut writes = 0;
    for (key, trackable) in all.iter() {
        let state_str = format!("{:?}", trackable.state);
        if state_str == "None" || state_str == "NotFound" {
            continue;
        }
        if key.id == -4 {
            continue; // LedgerContract excluded from trie
        }
        writes += 1;
        let val = trackable.item.value_bytes();
        eprintln!(
            "  id={} key={} state={} value=({}B) {}",
            key.id,
            hex::encode(key.key()),
            state_str,
            val.len(),
            hex::encode(&val),
        );
    }
    eprintln!("total writes (non-ledger): {writes}");

    let mut applied = 0usize;
    let mut trie_guard = trie.lock();
    for (key, trackable) in all.iter() {
        let state_str = format!("{:?}", trackable.state);
        if state_str == "None" || state_str == "NotFound" {
            continue;
        }
        if key.id == -4 {
            continue;
        }
        let key_bytes = key.to_array();
        match state_str.as_str() {
            "Added" | "Changed" => {
                trie_guard
                    .put(&key_bytes, &trackable.item.value_bytes())
                    .expect("trie.put");
                applied += 1;
            }
            "Deleted" => {
                trie_guard.delete(&key_bytes).expect("trie.delete");
                applied += 1;
            }
            _ => {}
        }
    }
    let new_root = trie_guard.root_hash();
    drop(trie_guard);

    let expected = u256("0xd84d614a7b4e5ac08ec3d834cc680831f05fc561ea5e9b5892471fb51e52248e");
    eprintln!("\napplied={applied} new_root={new_root:?}");
    eprintln!("expected_csharp_root={}", expected);
    assert_eq!(
        new_root,
        Some(expected),
        "new state root must match C# expected at block 1,465,790",
    );
}
