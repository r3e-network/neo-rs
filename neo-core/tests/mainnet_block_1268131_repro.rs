#![cfg(feature = "rocksdb")]

// Reproducer for block 1,268,131 — Bug #11 (OPEN). State root divergence first
// appears at this block (bisect via seed1.neo.org getstateroot vs local Rust trie
// after bug #10 fix landed). Single tx 0x2c11be50... calls `offline_mint(epoch_id,
// account)` on the puppet NFT contract 0x76a8f8a7a901b29a33013b469949f4b08db15756.
// C# applog: HALT, returns ByteString "1", emits Transfer(Null, sender, 1, "1").
// Rust: FAULTs. The signer scope is CalledByEntry (1). Cause TBD.
//
// C# expected state root @ 1,268,131: 0xdf0f41ed960f56d2644b26a126c6a240f8c1201fec6e501095fcdb13528ebb42
// C# expected state root @ 1,268,130: 0x547a380ccd2409657a9f080c7f8706a484281d0fed5b9053276050db4dbcdc85

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use neo_core::ledger::{Block, BlockHeader};
use neo_core::network::p2p::payloads::{
    signer::Signer, transaction::Transaction, witness::Witness,
};
use neo_core::persistence::data_cache::{DataCache, DataCacheConfig};
use neo_core::persistence::{i_store_provider::IStoreProvider, providers::RocksDBStoreProvider};
use neo_core::persistence::{SeekDirection, StorageConfig};
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::native::ledger_contract::PersistedTransactionState;
use neo_core::smart_contract::native::LedgerTransactionStates;
use neo_core::smart_contract::trigger_type::TriggerType;
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

const TX_SCRIPT: &str = include_str!("mainnet_block_1268131_data/tx_script.b64");
const TX_INV: &str = include_str!("mainnet_block_1268131_data/tx_inv.b64");
const TX_VER: &str = include_str!("mainnet_block_1268131_data/tx_ver.b64");

#[test]
#[ignore = "requires local mainnet full-state data synced past height 1,268,130"]
fn replay_block_1268131_assert_csharp_root() {
    let state_store = open_state_store();
    let Some(root_prev) = state_store.get_state_root(1_268_130).map(|r| r.root_hash) else {
        eprintln!("[SKIPPED] state root 1268130 not present in StateRoot DB.");
        return;
    };
    eprintln!("loaded prior state root @1268130 = {}", root_prev);
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
    tx.set_nonce(1_133_962_737);
    tx.set_system_fee(40_436_620);
    tx.set_network_fee(120_852);
    tx.set_valid_until_block(1_273_890);
    let tx_signer = Signer::new(
        u160("0xdfee4286928e1e13fddff2ff003960c7ec74c0a0"),
        neo_core::WitnessScope::CALLED_BY_ENTRY,
    );
    tx.set_signers(vec![tx_signer]);
    tx.set_attributes(Vec::new());
    tx.set_script(BASE64.decode(TX_SCRIPT.trim()).expect("tx script"));
    tx.set_witnesses(vec![witness(TX_INV.trim(), TX_VER.trim())]);

    let header = BlockHeader::new(
        0,
        u256("0xc9d38361eaa0306a335ae249428108c2833ec481e2bedd96a6b8f2a158278794"),
        u256("0xe78bab8813007a2a3114dc7974f03fea5e5666e2e66831bf6faebc4150be112c"),
        1_647_991_059_624,
        14_064_156_275_055_279_441,
        1_268_131,
        4,
        u160("0x682cca3ebdc66210e5847d7f8115846586079d4a"),
        vec![],
    );
    let block = Arc::new(Block::new(header, vec![tx.clone()]));

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
        neo_core::neo_vm::VMState::HALT,
        "tx must HALT (matches C# applog: HALT, gas=40436620, returns \"1\", emits Transfer)"
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
    let new_root = trie_guard.root_hash();
    drop(trie_guard);

    let expected = u256("0xdf0f41ed960f56d2644b26a126c6a240f8c1201fec6e501095fcdb13528ebb42");
    eprintln!("\napplied={applied} new_root={new_root:?}");
    assert_eq!(
        new_root,
        Some(expected),
        "new state root must match C# expected at block 1,268,131"
    );
}
