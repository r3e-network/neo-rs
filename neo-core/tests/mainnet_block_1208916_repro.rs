#![cfg(feature = "rocksdb")]

// Reproducer for block 1,208,916 — Bug #10. State root divergence first appears
// at this block (bisect via seed1.neo.org getstateroot vs local Rust trie).
// Single tx 0xe707770e... calls FTWSmith factory contract's `createNEP11` to
// deploy "Three Orange Hearts" (TOH) NEP-11 NFT. The bug is in
// `ContractManifest::to_stack_item()` which used raw `serde_json::to_string()`
// for the manifest's `extra` field — serde_json's minimal RFC-8259 escape set
// differs from C#'s `JavaScriptEncoder.Default` (which escapes `&`, `<`, `>`,
// `'`, `+`, `` ` ``, all non-ASCII). The TOH description "NEO, GAS, & FLM on
// Neo N3" contains `&`, so Rust stored `&` literally while C# stored `&`.
// Fix: use `JsonSerializer::encode_value_csharp_compatible` (the helper
// extracted from the bug #9 escape encoder).
//
// C# expected state root @ 1,208,916: 0x27e7aee71c4cc24bb1f06c2fa8956816da78e1e7725ee09739780f345580c494

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

const TX_SCRIPT: &str = include_str!("mainnet_block_1208916_data/tx_script.b64");
const TX_INV: &str = include_str!("mainnet_block_1208916_data/tx_inv.b64");
const TX_VER: &str = include_str!("mainnet_block_1208916_data/tx_ver.b64");

#[test]
#[ignore = "requires local mainnet full-state data synced past height 1,208,915"]
fn replay_block_1208916_assert_csharp_root() {
    let state_store = open_state_store();
    let Some(root_prev) = state_store.get_state_root(1_208_915).map(|r| r.root_hash) else {
        eprintln!("[SKIPPED] state root 1208915 not present in StateRoot DB.");
        return;
    };
    eprintln!("loaded prior state root @1208915 = {}", root_prev);
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

    // Build the single tx
    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(1_189_337_400);
    tx.set_system_fee(1_005_406_541);
    tx.set_network_fee(142_802);
    tx.set_valid_until_block(1_208_945);
    let mut tx_signer = Signer::new(
        u160("0x2a4b6039b7ba84c0b313e157793bd41859427299"),
        neo_core::WitnessScope::CUSTOM_CONTRACTS,
    );
    tx_signer.allowed_contracts = vec![
        u160("0xf860bbdcc5091a9ae7812b047fd325dfa7905ee1"),
        u160("0xd2a4cff31913016155e38e474a2c06d08be276cf"),
    ];
    tx.set_signers(vec![tx_signer]);
    tx.set_attributes(Vec::new());
    tx.set_script(BASE64.decode(TX_SCRIPT.trim()).expect("tx script"));
    tx.set_witnesses(vec![witness(TX_INV.trim(), TX_VER.trim())]);

    let header = BlockHeader::new_with_witnesses(
        0,
        u256("0xd0fc0d09c1a3d083126c37b7ce0dffba901975cb71f568eec979f7e6118ebdf1"),
        u256("0xa34897145bd5ba320365b26a05d5cd0a0145da8d2db56fe7311cc12b0e7707e7"),
        1_647_033_331_120,
        18_208_040_181_683_027_309,
        1_208_916,
        2,
        u160("0x682cca3ebdc66210e5847d7f8115846586079d4a"),
        vec![],
    );
    let block = Arc::new(Block::from_parts(header, vec![tx.clone()]));

    // OnPersist
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
    let mut tx_states = on_persist_engine
        .take_state::<LedgerTransactionStates>()
        .unwrap_or_else(|| {
            LedgerTransactionStates::new(vec![PersistedTransactionState::new(&tx, block.index())])
        });
    drop(on_persist_engine);

    // Tx execution
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
        seeded_contracts.clone(),
        Arc::clone(&seeded_native_cache),
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
    eprintln!("\n=== tx result ===\nvm_state={vm_state:?} gas={gas} exception={exception:?}");
    eprintln!("notifications ({}):", notifs.len());
    for (i, n) in notifs.iter().take(20).enumerate() {
        eprintln!(
            "  [{}] contract={} event={}",
            i, n.script_hash, n.event_name
        );
    }
    if notifs.len() > 20 {
        eprintln!("  ... +{} more", notifs.len() - 20);
    }
    assert_eq!(
        vm_state,
        neo_vm_rs::VmState::HALT,
        "tx must HALT (matches C# applog)"
    );

    tx_states = tx_engine
        .take_state::<LedgerTransactionStates>()
        .unwrap_or_else(|| LedgerTransactionStates::new(Vec::new()));

    let tracked: Vec<_> = tx_snapshot.tracked_items().into_iter().collect();
    base_cache.merge_tracked_items(&tracked);

    // PostPersist
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

    // Apply tracked changes to trie and compute new root
    let mut applied = 0usize;
    let mut all: Vec<_> = base_cache.tracked_items().into_iter().collect();
    all.sort_by(|a, b| (a.0.id, a.0.key()).cmp(&(b.0.id, b.0.key())));
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
    let new_root = trie_guard
        .root_hash()
        .unwrap_or_else(neo_core::UInt256::zero);
    eprintln!("\napplied={} new_root={}", applied, new_root);

    // C# expected state root @ 1,208,916 (from seed1.neo.org getstateroot).
    let expected = u256("0x27e7aee71c4cc24bb1f06c2fa8956816da78e1e7725ee09739780f345580c494");
    assert_eq!(
        new_root, expected,
        "block 1,208,916 state root must match C# v3.9.1 after bug #10 fix"
    );
}
