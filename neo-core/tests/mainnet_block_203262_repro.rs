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

fn tx_203262() -> Transaction {
    let mut signer = Signer::new(
        u160("0x3a11aec89f04e29ae36b346ccddc3e04cd7e72b6"),
        neo_core::WitnessScope::CUSTOM_CONTRACTS,
    );
    signer.allowed_contracts = vec![
        u160("0xcc638d55d99fc81295daccbaf722b84f179fb9c4"),
        u160("0x577a51f7d39162c9de1db12a6b319c848e4c54e5"),
        u160("0xd2a4cff31913016155e38e474a2c06d08be276cf"),
    ];

    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(3_072_711_006);
    tx.set_system_fee(30_144_110);
    tx.set_network_fee(1_327_520);
    tx.set_valid_until_block(203_292);
    tx.set_signers(vec![signer]);
    tx.set_attributes(Vec::new());
    tx.set_script(
        BASE64
            .decode("AmAKOkIMAs0ADBS2cn7NBD7czWw0a+Oa4gSfyK4ROhPAHwwIYmlkVG9rZW4MFMS5nxdPuCL3uszalRLIn9lVjWPMQWJ9W1I=")
            .expect("tx script"),
    );
    tx.set_witnesses(vec![witness(
        "DEAzX3cCRO/rCRqMSCLrFPzWbtrBsBE8HYHaHGRwQaQs0lJT+p9vEWqOvDrfsS4qQJ+agvxSVVO+IRixcTW/jfGR",
        "DCEDkUKWm1X8W0EEKXInV//4cq1ZZRczDofc1w5l3N3aBy5BVuezJw==",
    )]);
    tx
}

fn block_203262(tx: Transaction) -> Block {
    let header = BlockHeader::new(
        0,
        u256("0xf862ec72f135b8cd6a40004efc2dbda09382f53bfc977bf92e160249500c4957"),
        u256("0x7cb8524fe4710d5af4fe365977b3339713a438b950e54f12d70e3106bb045b4e"),
        1_631_048_459_492,
        u64::from_str_radix("E7F8CF0F1E46FA39", 16).expect("nonce"),
        203_262,
        3,
        UInt160::from_address("NSiVJYZej4XsxG5CUpdwn7VRQk8iiiDMPM").expect("next consensus"),
        vec![witness(
            "DECUK+Y7ThVOE8olS/kR+gwqUaw0yHKbxO0BtOIcfksWBgEvyi8yWkzj5Sri98rk4203dKhGRiWMbD7d/lLKugn5DEDNVkJFLDaCbS42a0B+dvvhyo6Lm785Vt/MhJ8vTv9+Jg5wtIlvigH25/IAMGoyvSBTIN05suWMwmvuEWU/CRsTDEAv49XxfTv0gHrobJE29VH7P6UjoRNEoYioH7geeOihIl9ddM5lhuanPQ/Lk6ppT1MGXiiMrxEE3TfJxqvdHN5dDEBRlF+WiWq8GuBQvX13IYvoJqh9o28ebHEY2l8yl4/lRc7afvtDyZx/ckVDBdUH4UZ+YZbDWLgLEKBgdT0z+i3sDEBnumvcMyXKV7gkccdm4w3LdIOf6N6tJI71POG442p1FsEagRlmmJ52K4QsUHIcXFlK5v0+k/ImTb5SY/cM1DNW",
            "FQwhAjmjdDZlL0GzuALKRMvLfWXTqguIyaA4AkO9vhqqXLNbDCECSG/RVwLESQomcDESpcwdCSP9aXozQGvVocAOABOwmnAMIQKq7DhHD2qtAELG6HfP2Ah9Jnaw9Rb93TYoAbm9OTY5ngwhA7IJ/U9TpxcOpERODLCmu2pTwr0BaSaYnPhfmw+6F6cMDCEDuNnVdx2PUTqghpucyNUJhkA7eMbaNokGOMPUalrc4EoMIQLKDidpe5wkj28W4IX9AGHib0TahbWO6DXBEMql7DulVAwhA9nosWvZsi0zRdbUzeMb4cPh0WFTLj0MzsuV7OLrWDNuF0Ge0Nw6",
        )],
    );

    Block::new(header, vec![tx])
}

fn open_state_store() -> StateStore {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .to_path_buf();
    // Active DB doesn't currently have heights ≥200,000. Set
    // NEO_REPRO_DB_PATH=data/mainnet.pre-274157-fix-20260419/StateRoot
    // to point at a richer backup that does have block 203,261.
    let state_root_path = match std::env::var("NEO_REPRO_DB_PATH").ok() {
        Some(p) if !p.is_empty() => Path::new(&p).to_path_buf(),
        _ => repo_root.join("data/mainnet/StateRoot"),
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

#[test]
#[ignore = "requires local mainnet full-state data under ./data/mainnet/StateRoot"]
fn replay_block_203262_against_root_203261() {
    let state_store = open_state_store();
    let Some(root_203261) = state_store.get_state_root(203_261).map(|r| r.root_hash) else {
        eprintln!(
            "[SKIPPED] mainnet_block_203262_repro: state root 203261 not present. \
             Set NEO_REPRO_DB_PATH=data/mainnet.pre-274157-fix-20260419/StateRoot \
             to point at a backup that has block 203,261. Reported as PASS but \
             assertions did NOT run."
        );
        return;
    };
    let trie = Arc::new(Mutex::new(state_store.trie_for_root(root_203261)));

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

    let tx = tx_203262();
    let block = Arc::new(block_203262(tx.clone()));

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
        seeded_contracts.clone(),
        Arc::clone(&seeded_native_cache),
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

    assert_eq!(vm_state, neo_core::neo_vm::VMState::FAULT);
    let fault_msg = tx_engine
        .fault_exception()
        .map(|s| s.to_string())
        .unwrap_or_default();
    assert!(
        fault_msg.contains("Gas exhausted") || fault_msg.contains("Insufficient GAS"),
        "expected gas-exhaustion fault, got: {:?}",
        fault_msg,
    );

    // FAULT path: tx_snapshot writes are discarded (no merge into base_cache).
    let tx_states = tx_engine
        .take_state::<LedgerTransactionStates>()
        .unwrap_or_else(|| LedgerTransactionStates::new(Vec::new()));
    drop(tx_engine);
    drop(tx_snapshot);

    // Run PostPersist on base_cache (mirrors persist_block_internal).
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
    post_persist_engine.native_post_persist().expect("post persist");
    drop(post_persist_engine);

    // Apply non-Ledger storage changes to trie and assert root matches C#.
    let mut applied = 0usize;
    let mut skipped_ledger = 0usize;
    let mut all: Vec<_> = base_cache.tracked_items().into_iter().collect();
    all.sort_by(|a, b| (a.0.id, a.0.key()).cmp(&(b.0.id, b.0.key())));
    let mut trie_guard = trie.lock();
    for (key, trackable) in all.iter() {
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
    eprintln!(
        "applied={} skipped_ledger={} new_root={}",
        applied, skipped_ledger, new_root
    );
    let expected_csharp_root = UInt256::parse(
        "0x0febf7e861702ec0491e59938a7f76baf9c850f6c9ea25635c7b6f23e798fe46",
    )
    .expect("parse expected C# root");
    assert_eq!(
        new_root, expected_csharp_root,
        "block 203262 OnPersist + FAULT-tx + PostPersist state root must match C# v3.9.1",
    );
}
