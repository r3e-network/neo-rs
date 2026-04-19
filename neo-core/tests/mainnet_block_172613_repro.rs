#![cfg(feature = "rocksdb")]

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

fn tx_172613() -> Transaction {
    let signer = Signer::new(
        u160("0x56455490d5a711746801f954345552ea03824b29"),
        neo_core::WitnessScope::CALLED_BY_ENTRY,
    );

    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(608_090_061);
    tx.set_system_fee(9_977_750);
    tx.set_network_fee(661_760);
    tx.set_valid_until_block(172_852);
    tx.set_signers(vec![signer]);
    tx.set_attributes(Vec::new());
    tx.set_script(
        BASE64
            .decode("CwENAgwU2/NiyHzl96UBpV6XuF98vM+C9j4MFClLggPqUlU0VPkBaHQRp9WQVEVWFMAfDAh0cmFuc2ZlcgwU9WPqQLwoPU0OBcSOowWz8qBzQO9BYn1bUgsDGdSuYwUAAAAMFNvzYsh85felAaVel7hffLzPgvY+DBQpS4ID6lJVNFT5AWh0EafVkFRFVhTAHwwIdHJhbnNmZXIMFM924ovQBixKR47jVWEBExnzz6TSQWJ9W1I=")
            .expect("tx script"),
    );
    tx.set_witnesses(vec![witness(
        "DEA/LjRDQz5ZFBPVdp1CXYpwVqxtCTbq2nnKTkQREeskaETahcghzE2W7VpWy7zSoc9vdnnWADTImSj1Zyj+SzI+",
        "DCECcCBOlRTJboIjs+ZDa3mu4PKIn1OLPDZV1LtmUwUf5pxBVuezJw==",
    )]);
    tx
}

fn block_172613(tx: Transaction) -> Block {
    let header = BlockHeader::new(
        0,
        u256("0x8a346c15ddeec780d9c2dec6bd683d14c349b9acad66173630a57a39a6ee4ce9"),
        u256("0x31f7a45c260e79b80d54cca8b653460de1671eef7b0085a6bebc2c7f9b69c724"),
        1_630_572_494_751,
        u64::from_str_radix("809B62882F08C0D8", 16).expect("nonce"),
        172_613,
        0,
        UInt160::from_address("NYkw1YaCPHB4BoTFDdLYMXXG84P6xKd8cz").expect("next consensus"),
        vec![],
    );

    Block::new(header, vec![tx])
}

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
#[ignore = "requires local mainnet full-state data under ./data/mainnet/StateRoot"]
fn replay_block_172613_against_root_172612() {
    let state_store = open_state_store();
    let root_172612 = state_store
        .get_state_root(172_612)
        .expect("state root 172612 present")
        .root_hash;
    let trie = Arc::new(Mutex::new(state_store.trie_for_root(root_172612)));

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
            move |prefix: Option<&neo_core::smart_contract::StorageKey>,
                  _direction: SeekDirection| {
                let prefix_bytes = prefix
                    .map(|key| key.to_array().to_owned())
                    .unwrap_or_default();
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

    let tx = tx_172613();
    let block = Arc::new(block_172613(tx.clone()));

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
            LedgerTransactionStates::new(vec![PersistedTransactionState::new(&tx, block.index())])
        });

    let tx_store_get = {
        let base = Arc::clone(&base_cache);
        Arc::new(move |key: &neo_core::smart_contract::StorageKey| base.get(key))
    };
    let tx_store_find = {
        let base = Arc::clone(&base_cache);
        Arc::new(
            move |prefix: Option<&neo_core::smart_contract::StorageKey>,
                  direction: SeekDirection| {
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
        seeded_contracts,
        seeded_native_cache,
        None,
    )
    .expect("tx engine");
    tx_engine.set_state(tx_states);
    tx_engine
        .load_script(tx.script().to_vec(), CallFlags::ALL, None)
        .expect("load tx script");

    let vm_state = tx_engine.execute_allow_fault();
    eprintln!(
        "vm_state={vm_state:?} gas_consumed={} fee_consumed={} exception={:?}",
        tx_engine.gas_consumed(),
        tx_engine.fee_consumed(),
        tx_engine.fault_exception(),
    );

    eprintln!("result stack:");
    for (i, item) in tx_engine.result_stack().iter().enumerate() {
        eprintln!("  [{}] {:?}", i, item);
    }

    eprintln!("notifications ({}):", tx_engine.notifications().len());
    for (i, notif) in tx_engine.notifications().iter().enumerate() {
        eprintln!("  [{}] contract={} event={}", i, notif.script_hash, notif.event_name);
        for item in &notif.state {
            eprintln!("      {:?}", item);
        }
    }

    // Merge tx changes to base_cache on HALT
    if vm_state == neo_core::neo_vm::VMState::HALT {
        let tracked = tx_snapshot.tracked_items();
        base_cache.merge_tracked_items(&tracked);
    }

    // Then post_persist
    let mut post_persist_engine = ApplicationEngine::new_with_preloaded_native(
        TriggerType::PostPersist,
        None,
        Arc::clone(&base_cache),
        Some(Arc::clone(&block)),
        ProtocolSettings::mainnet(),
        neo_core::smart_contract::application_engine::TEST_MODE_GAS,
        tx_engine.contracts_snapshot(),
        tx_engine.native_contract_cache_handle(),
        None,
    )
    .expect("post persist engine");
    post_persist_engine.native_post_persist().expect("post persist");

    // Compare sender's GAS balance
    let sender_gas_key = hex::decode("faffffff14294b8203ea52553454f901687411a7d590544556").unwrap();
    let sender_gas_sk = neo_core::smart_contract::StorageKey::from_bytes(&sender_gas_key);
    let sender_gas = base_cache.get(&sender_gas_sk);
    eprintln!(
        "\n==== sender GAS balance key ===\n  ours: {}\n  C#  : 41012105af0a315d05",
        sender_gas.as_ref().map(|v| hex::encode(v.value_bytes())).unwrap_or("<none>".into())
    );

    // Apply all non-Ledger changes to the trie and compute new root
    eprintln!("\n=== Applying changes to 172,612 trie to get 172,613 root ===");
    let tracked = base_cache.tracked_items();
    let mut applied = 0usize;
    let mut skipped_ledger = 0usize;
    let mut trie_guard = trie.lock();
    for (key, trackable) in &tracked {
        let state_str = format!("{:?}", trackable.state);
        if state_str == "None" || state_str == "NotFound" {
            continue;
        }
        if key.id == -4 {
            skipped_ledger += 1;
            continue;
        }
        let key_bytes = key.to_array();
        match state_str.as_str() {
            "Added" | "Changed" => {
                trie_guard.put(&key_bytes, &trackable.item.value_bytes()).expect("trie.put");
                applied += 1;
            }
            "Deleted" => {
                trie_guard.delete(&key_bytes).expect("trie.delete");
                applied += 1;
            }
            _ => {}
        }
    }
    let new_root = trie_guard.root_hash().unwrap_or_else(neo_core::UInt256::zero);
    eprintln!("applied={} skipped_ledger={}", applied, skipped_ledger);
    eprintln!("our computed root: {}", new_root);
    eprintln!("stored our root:   0xa512a1ce9f29dfdbf93d456b2f57de441ee968c22daf0ba137c9167188c270b5");
    eprintln!("C# root:           0x6493dab848dce8bce8c1196e61365dba872d1efa0406f06ff3fc964318680e47");
}
