#![cfg(feature = "rocksdb")]

// Reproducer for block 980,196 divergence (eighth post-fix bug).
//
// Block 980,196 has 2 transactions on mainnet (C# Neo v3.9.1):
//   tx0 0xfa70eb1612f53b8e23bf427a7eb8e6c1a2971b5d5a194db620f13d1c30b4cbc9
//       sender 0xcb3817b6847443d8a99fa6a559a9f6d38cba5e93 (scope=CalledByEntry,
//       LE 0x935eba8cd3f6a959a5a69fa9d8437484b61738cb)
//       sysfee = 10,774,598 GAS (1.07 GAS); HALT in C#, FAULT in Rust
//       Calls NEO.transfer(sender, n3trader, 1, 90) followed by
//             0x88da18a5...transfer(sender, n3trader, ~10^13, 90)
//       Both expected to HALT in C#. Receiver's onNEP17Payment callback
//       does Runtime.CheckWitness(sender) which is the suspected divergence point.
//
//   tx1 0x196ed96c46486022f7327bfbb6be1b0e0bd6fe647e42dcfa4c05fcb5446e0122
//       sender 0xdd4a4985649681b77522262e0f75af7b4baecb4a (scope=CustomContracts)
//       sysfee = 6,009,676; HALT
//       Mints a NeoAnts NFT (one-of-one with embedded JSON metadata).
//
// Our Rust sync produces wrong state root at 980,196:
//   Rust: 0x03c11735ccd8133acd98e3e96e9d56d28aaeb8080c865e34f81aa783e354135d
//   C#:   0xdf3e98ea20e700c91fce1b5f273891c499b70da890b670c0ab71722624072682
//
// This reproducer replays just block 980,196 against the local DB at the
// 980,195 snapshot and asserts the resulting state root equals C#.

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
#[ignore = "requires local mainnet full-state data synced past height 980,195"]
fn replay_block_980196_debug() {
    let state_store = open_state_store();
    let Some(root_980195) = state_store.get_state_root(980_195).map(|r| r.root_hash) else {
        eprintln!(
            "[SKIPPED] mainnet_block_980196_repro: state root 980195 not present in \
             data/mainnet/StateRoot. This test is a no-op until the local DB has been \
             synced past height 980_195."
        );
        return;
    };
    eprintln!("loaded prior state root @980195 = {}", root_980195);
    let trie = Arc::new(Mutex::new(state_store.trie_for_root(root_980195)));

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

    // Build tx0: NEO.transfer + 0x88da18a5.transfer (the diverging one)
    let mut tx0 = Transaction::new();
    tx0.set_version(0);
    tx0.set_nonce(1_622_545_692);
    tx0.set_system_fee(10_774_598);
    tx0.set_network_fee(132_462);
    tx0.set_valid_until_block(980_226);
    let tx0_signer = Signer::new(
        u160("0xcb3817b6847443d8a99fa6a559a9f6d38cba5e93"),
        neo_core::WitnessScope::CALLED_BY_ENTRY,
    );
    tx0.set_signers(vec![tx0_signer]);
    tx0.set_attributes(Vec::new());
    tx0.set_script(
        BASE64
            .decode("AFoRDBRjRvEL4vb+xYXdxUaa4nMY+aR4OQwUk166jNP2qVmlpp+p2EN0hLYXOMsUwB8MCHRyYW5zZmVyDBT1Y+pAvCg9TQ4FxI6jBbPyoHNA70FifVtSAFoDAKByThgJAAAMFGNG8Qvi9v7Fhd3FRpricxj5pHg5DBSTXrqM0/apWaWmn6nYQ3SEthc4yxTAHwwIdHJhbnNmZXIMFC9DWjXE0KdgSZttIMhuqLylGNqIQWJ9W1I=")
            .expect("tx0 script"),
    );
    tx0.set_witnesses(vec![witness(
        "DEBCu26W3senVz+F2Iir4bRUXVKp5aiZx9dPmTxk1beGki16JTNOpnUMeGlBPVXmMJpq+YdJ6+446rdv5gZJILS3",
        "DCECrP8BSAmSwqeLDhP/Brvx6N6SKh2fEwi/NVtKnOss+pZBVuezJw==",
    )]);

    // Build tx1: NeoAnts NFT mint
    let mut tx1 = Transaction::new();
    tx1.set_version(0);
    tx1.set_nonce(315_435_516);
    tx1.set_system_fee(6_009_676);
    tx1.set_network_fee(149_252);
    tx1.set_valid_until_block(980_226);
    let mut tx1_signer = Signer::new(
        u160("0xdd4a4985649681b77522262e0f75af7b4baecb4a"),
        neo_core::WitnessScope::CUSTOM_CONTRACTS,
    );
    tx1_signer.allowed_contracts = vec![
        u160("0xd2a4cff31913016155e38e474a2c06d08be276cf"),
        u160("0x577a51f7d39162c9de1db12a6b319c848e4c54e5"),
    ];
    tx1.set_signers(vec![tx1_signer]);
    tx1.set_attributes(Vec::new());
    tx1.set_script(
        BASE64
            .decode("DAAMAAwADPJ7Im5hbWUiOiJOZW9BbnRzIDEvMSIsImRlc2NyaXB0aW9uIjoiT25lIG9mIG9uZSBOZW9BbnRzIE5GVHMgdGhpcyB3aWxsIGJlIHRoZSBvbmx5IG9uZSBldmVyIG1pbnRlZCFcblxuIiwiaW1hZ2UiOiJpcGZzOi8vUW1lVTZ6eGJmN0VZQmU4TUF2TGpDaTJ3alBIa0JYMzlqZkhTTEdEaUROblVCWiIsInRva2VuVVJJIjoiIiwiYXR0cmlidXRlcyI6W10sInByb3BlcnRpZXMiOnsiaGFzX2xvY2tlZCI6ZmFsc2UsInR5cGUiOjJ9fQwUSsuuS3uvdQ8uJiJ1t4GWZIVJSt0VwB8MBG1pbnQMFOVUTI6EnDFrKrEd3slikdP3UXpXQWJ9W1I=")
            .expect("tx1 script"),
    );
    tx1.set_witnesses(vec![witness(
        "DECfZy8Bw9K20Ij1EuXKATxX8lzkU4qPLt5l2ML265o2p7sW0Cs2qpkE1uVEnoX2tFgnUZOkuiXJWEwHZuaM21m9",
        "DCEDnZlwb9cvIZyeMTZA5XUq5i4lubzHP8ivHSeTA4MAwp5BVuezJw==",
    )]);

    // Block 980,196 from mainnet RPC getblock
    let header = BlockHeader::new(
        0,
        u256("0x93d5800ec22a9738c930bc369cbae5e7096d6354b775fea8935949eb0531aae1"),
        u256("0x0000000000000000000000000000000000000000000000000000000000000000"),
        1_643_330_248_667,
        0x9BE85A6E185F07D3,
        980_196,
        0,
        UInt160::from_address("NSiVJYZej4XsxG5CUpdwn7VRQk8iiiDMPM").expect("nextconsensus"),
        vec![],
    );
    let block = Arc::new(Block::new(header, vec![tx0.clone(), tx1.clone()]));

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
            LedgerTransactionStates::new(vec![
                PersistedTransactionState::new(&tx0, block.index()),
                PersistedTransactionState::new(&tx1, block.index()),
            ])
        });
    drop(on_persist_engine);

    for (idx, tx) in [&tx0, &tx1].iter().enumerate() {
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
            Some(Arc::new((*tx).clone())),
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
        {
            let exception = tx_engine.fault_exception();
            let notifs = tx_engine.notifications();
            eprintln!(
                "\n=== tx{} result ===\nvm_state={vm_state:?} gas={gas} exception={exception:?}",
                idx,
            );
            eprintln!("notifications ({}):", notifs.len());
            for (i, n) in notifs.iter().enumerate() {
                eprintln!(
                    "  [{}] contract={} event={}",
                    i, n.script_hash, n.event_name
                );
            }
        }

        tx_states = tx_engine
            .take_state::<LedgerTransactionStates>()
            .unwrap_or_else(|| LedgerTransactionStates::new(Vec::new()));

        // tx0 must HALT for the C# state root to match. If it FAULTs we have repro'd bug #8.
        if vm_state != neo_vm_rs::VmState::HALT {
            eprintln!(
                "\n!!! tx{} FAULTed at block 980,196 — bug #8 reproduced (gas={gas}) !!!",
                idx,
            );
        }

        let tracked: Vec<_> = tx_snapshot.tracked_items().into_iter().collect();
        base_cache.merge_tracked_items(&tracked);
    }

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
        "\napplied={} skipped_ledger={} new_root={}",
        applied, skipped_ledger, new_root
    );

    // C# expected root for block 980,196.
    let expected_csharp_root =
        UInt256::parse("0xdf3e98ea20e700c91fce1b5f273891c499b70da890b670c0ab71722624072682")
            .expect("parse expected C# root");
    assert_eq!(
        new_root, expected_csharp_root,
        "block 980196 OnPersist + 2-tx + PostPersist state root must match C# v3.9.1",
    );
}
