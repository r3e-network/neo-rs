#![cfg(feature = "rocksdb")]

// Reproducer for block 1,283,521 — Bug #10 hunt.
//
// Block 1,283,521 has a single transaction:
//   tx 0xd52a392671690cf4e824e52bdcc03261fec63d7665cc87ba108233debe028c74
//   sender: 0x673a663cebe612f6e63e9bf85a2076d601fe5fb9
//   sysfee=997775, netfee=123152
//   script: GAS.transfer(from=0x673a663c..., to=0xe69f64c8..., 1900100000=19 GAS, null)
//
// In iter61 (live sync), this tx HALTs successfully and the SENDER's GAS balance is
// debited correctly (post-block: 1.155 GAS) — but the RECEIVER's GAS storage entry
// is MISSING from the state. This causes block 1,283,522 (where receiver is sender of
// tx 0xa2b88a6e) to fail with InsufficientFunds during OnPersist's gas burn.
//
// This reproducer replays block 1,283,521 against the trie snapshot at 1,283,520
// and prints every tracked storage write, with special attention to the receiver's
// GAS account key. Run with NEO_GAS_WATCH_ACCOUNT=0xe69f64c8fa57c7b23a2c75f4b234c030993dc39b
// to also get per-account watch logs from gas_token.rs.

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

const TX_SCRIPT: &str = include_str!("mainnet_block_1283521_data/tx_script.b64");
const TX_INV: &str = include_str!("mainnet_block_1283521_data/tx_inv.b64");
const TX_VER: &str = include_str!("mainnet_block_1283521_data/tx_ver.b64");

const RECEIVER_HEX: &str = "0xe69f64c8fa57c7b23a2c75f4b234c030993dc39b";
const SENDER_HEX: &str = "0x673a663cebe612f6e63e9bf85a2076d601fe5fb9";

#[test]
#[ignore = "requires local mainnet full-state data synced past height 1,283,520"]
fn replay_block_1283521_debug() {
    let state_store = open_state_store();
    let Some(root_prev) = state_store.get_state_root(1_283_520).map(|r| r.root_hash) else {
        eprintln!(
            "[SKIPPED] state root 1283520 not present in StateRoot DB. \
             Set NEO_REPRO_DB_PATH=/path/to/StateRoot if needed."
        );
        return;
    };
    eprintln!("loaded prior state root @1283520 = {}", root_prev);
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

    // Pre-replay: confirm receiver has no GAS entry
    let receiver = u160(RECEIVER_HEX);
    let receiver_key = neo_core::smart_contract::StorageKey::create_with_uint160(-6, 20, &receiver);
    let pre_state = base_cache.get(&receiver_key);
    eprintln!(
        "PRE-REPLAY receiver GAS entry: {}",
        pre_state
            .as_ref()
            .map(|state| format!("{} bytes", state.value_bytes().len()))
            .unwrap_or_else(|| "<missing>".to_string())
    );
    let sender = u160(SENDER_HEX);
    let sender_key = neo_core::smart_contract::StorageKey::create_with_uint160(-6, 20, &sender);
    let pre_sender = base_cache.get(&sender_key);
    eprintln!(
        "PRE-REPLAY sender GAS entry: {}",
        pre_sender
            .as_ref()
            .map(|i| format!("0x{}", hex::encode(i.value_bytes())))
            .unwrap_or_else(|| "<missing>".to_string())
    );

    // Build the single tx
    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(2_863_850_176);
    tx.set_system_fee(997_775);
    tx.set_network_fee(123_152);
    tx.set_valid_until_block(1_283_550);
    let tx_signer = Signer::new(sender, neo_core::WitnessScope::CALLED_BY_ENTRY);
    tx.set_signers(vec![tx_signer]);
    tx.set_attributes(Vec::new());
    tx.set_script(BASE64.decode(TX_SCRIPT.trim()).expect("tx script"));
    tx.set_witnesses(vec![witness(TX_INV.trim(), TX_VER.trim())]);

    let header = BlockHeader::new(
        0,
        u256("0x023510b0e1ac92080e6ae38c801be153e520e850c4312026c63f5cf86e83ee9a"),
        u256("0x748c02bede338210ba87cc65763dc6fe6132c0dc2be524e8f40c697126392ad5"),
        1_648_240_916_669,
        4_993_611_658_313_286_282,
        1_283_521,
        1,
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
    eprintln!("OnPersist completed");

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
        .expect("load script");

    let vm_state = tx_engine.execute_allow_fault();
    let gas = tx_engine.gas_consumed();
    let exception = tx_engine.fault_exception();
    let notifs = tx_engine.notifications();
    eprintln!("\n=== tx result ===\nvm_state={vm_state:?} gas={gas} exception={exception:?}");
    eprintln!("notifications ({}):", notifs.len());
    for (i, n) in notifs.iter().enumerate() {
        eprintln!(
            "  [{}] contract={} event={} args_len={}",
            i,
            n.script_hash,
            n.event_name,
            n.state.len()
        );
    }

    tx_states = tx_engine
        .take_state::<LedgerTransactionStates>()
        .unwrap_or_else(|| LedgerTransactionStates::new(Vec::new()));

    let tracked: Vec<_> = tx_snapshot.tracked_items().into_iter().collect();
    eprintln!("\ntx-level tracked items count: {}", tracked.len());
    for (key, t) in tracked.iter() {
        let state_str = format!("{:?}", t.state);
        let key_bytes = key.to_array();
        let key_hex = hex::encode(&key_bytes);
        // Highlight receiver key
        let highlight =
            if key.id == -6 && key_hex.contains("9bc33d9930c034b2f4752c3ab2c757fac8649fe6") {
                " <<< RECEIVER GAS KEY"
            } else if key.id == -6 && key_hex.contains("b95ffe01d676205af89b3ee6f612e6eb3c663a67") {
                " <<< SENDER GAS KEY"
            } else {
                ""
            };
        eprintln!(
            "  state={} id={} keyhex=0x{} valhex=0x{}{}",
            state_str,
            key.id,
            key_hex,
            hex::encode(t.item.value_bytes()),
            highlight
        );
    }
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
    eprintln!("\nPostPersist completed");

    // Final inspection
    let post_state = base_cache.get(&receiver_key);
    eprintln!(
        "\nPOST-REPLAY receiver GAS entry: {}",
        post_state
            .as_ref()
            .map(|i| format!("0x{}", hex::encode(i.value_bytes())))
            .unwrap_or_else(|| "<missing>".to_string())
    );
    let post_sender = base_cache.get(&sender_key);
    eprintln!(
        "POST-REPLAY sender GAS entry: {}",
        post_sender
            .as_ref()
            .map(|i| format!("0x{}", hex::encode(i.value_bytes())))
            .unwrap_or_else(|| "<missing>".to_string())
    );

    if post_state.is_none() {
        eprintln!("\n!!! BUG #10 REPRODUCED: receiver GAS entry missing after replay !!!");
    } else {
        eprintln!("\n??? receiver entry present after replay — different from live sync ???");
    }
}
