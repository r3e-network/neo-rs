#![cfg(feature = "rocksdb")]

// Reproducer for block 679,779 divergence (fourth post-fix bug).
//
// Block 679,779 has 2 transactions, both HALT on mainnet (C# Neo v3.9.1):
//   tx0 0xf30773c435d87a4426714495a15b52157c12490fb183ad23ca6ca8c76cf33033
//       sender 0xfe2ad4e6a3dfcab9cf260227877fadb933dd9a15 (scope=CustomContracts)
//       sysfee = 6,092,076, gas matches; emits 1 Transfer notification on
//       contract 0x4d5a85b0c83777df72cfb665a933970e4e20c0ec (looks like a token transfer).
//
//   tx1 0x989b422873e0e5fd6078849646ef5f7f787f65270bb29b9bfdc39cf32407b972
//       sender NMPZugBZX26sdUXPT29WAwkHmDeUcXeToZ (= 0x33a7d61afbb73d3890143a30e2047af683573010)
//       (the SAME sender as tx0 of block 676,050)
//       sysfee = 16,388,720; HALT
//       Calls GAS.transfer(sender, n3trader, 1, ...)        — small transfer, NEP-17 callback
//                                                            triggers a NEP-11 (NFT) transfer
//                                                            from N3Trader back to sender,
//                                                            then more GAS transfers, then
//                                                            N3Trader emits TradeCompleted.
//       6 notifications: GAS Transfer×5, NFT Transfer (with data), TradeCompleted.
//
// Our Rust sync produces wrong state root at 679,779 (first divergence after the
// VM EQUAL type-strict fix at 676,050):
//   put_count=10, del_count=1
//
// This reproducer replays just block 679,779 against the local DB at the 679,778
// snapshot and asserts the resulting state root equals the C# expected value.

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
        BASE64.decode(verification_b64).expect("base64 verification"),
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
#[ignore = "requires local mainnet full-state data synced to 679,778 (clean) then applies block 679,779"]
fn replay_block_679779_debug() {
    let state_store = open_state_store();
    let Some(root_679778) = state_store.get_state_root(679_778).map(|r| r.root_hash) else {
        eprintln!(
            "[SKIPPED] mainnet_block_679779_repro: state root 679778 not present in \
             data/mainnet/StateRoot. This test is a no-op until the local DB has been \
             synced past height 679_778. Reported as PASS but assertions did NOT run."
        );
        return;
    };
    let trie = Arc::new(Mutex::new(state_store.trie_for_root(root_679778)));

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
                let prefix_bytes = prefix
                    .map(|k| k.to_array().to_owned())
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

    // Build tx0: token transfer
    let mut tx0 = Transaction::new();
    tx0.set_version(0);
    tx0.set_nonce(1_055_511_318);
    tx0.set_system_fee(6_092_076);
    tx0.set_network_fee(127_762);
    tx0.set_valid_until_block(679_809);
    let mut tx0_signer = Signer::new(
        u160("0xfe2ad4e6a3dfcab9cf260227877fadb933dd9a15"),
        neo_core::WitnessScope::CUSTOM_CONTRACTS,
    );
    tx0_signer.allowed_contracts = vec![
        u160("0xd1a9f78e1940f6322fef4df2340a963a9ec46f63"),
        u160("0x4d5a85b0c83777df72cfb665a933970e4e20c0ec"),
    ];
    tx0.set_signers(vec![tx0_signer]);
    tx0.set_attributes(Vec::new());
    tx0.set_script(
        BASE64
            .decode("CwPddYMABgAAAAwUY2/EnjqWCjTyTe8vMvZAGY73qdEMFBWa3TO5rX+HJwImz7nK36Pm1Cr+FMAfDAh0cmFuc2ZlcgwU7MAgTg6XM6llts9y33c3yLCFWk1BYn1bUg==")
            .expect("tx0 script"),
    );
    tx0.set_witnesses(vec![witness(
        "DEC8T8nw4Wb9tyKzm7KimlaujbUKkU4L98yqmirDjYhfZImIPRGqj+L6x1vZ7fYZeJucRxF1jVpLVwmzWKuGgHck",
        "DCEDFwXO/lPDiyWJDkmkVtRjec15DoUjNqLigaZ63cH5+XNBVuezJw==",
    )]);

    // Build tx1: N3Trader trade completion (the diverging tx)
    let mut tx1 = Transaction::new();
    tx1.set_version(0);
    tx1.set_nonce(3_881_824_749);
    tx1.set_system_fee(16_388_720);
    tx1.set_network_fee(131_862);
    tx1.set_valid_until_block(679_809);
    let tx1_signer = Signer::new(
        u160("0x33a7d61afbb73d3890143a30e2047af683573010"),
        neo_core::WitnessScope::CALLED_BY_ENTRY,
    );
    tx1.set_signers(vec![tx1_signer]);
    tx1.set_attributes(Vec::new());
    tx1.set_script(
        BASE64
            .decode("FREMFGNG8Qvi9v7Fhd3FRpricxj5pHg5DBQQMFeD9noE4jA6FJA4Pbf7GtanMxTAHwwIdHJhbnNmZXIMFM924ovQBixKR47jVWEBExnzz6TSQWJ9W1IVAoCWmAAMFGNG8Qvi9v7Fhd3FRpricxj5pHg5DBQQMFeD9noE4jA6FJA4Pbf7GtanMxTAHwwIdHJhbnNmZXIMFM924ovQBixKR47jVWEBExnzz6TSQWJ9W1I=")
            .expect("tx1 script"),
    );
    tx1.set_witnesses(vec![witness(
        "DEACQK2kCpIlCHLMXrGfi64rnKxCxCfEZel5Xe0otYkezpORVXPZ9obSOK5ODv/X2Lk6FbI/5nqv7eTB32+wCJA5",
        "DCECgDCKOy+uAaG9ybWHrFryynwyw5nWY2uVMZNMxBGHsnBBVuezJw==",
    )]);

    // Block 679,779 from mainnet RPC getblock
    let header = BlockHeader::new(
        0,
        u256("0xe8515731247c048ab07c96eb37dd03b97ca0983160271a67988784b35b16cc6d"),
        u256("0x0000000000000000000000000000000000000000000000000000000000000000"),
        1_638_519_520_023,
        0x2A5426E960E98AB2,
        679_779,
        2,
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
                eprintln!("  [{}] contract={} event={}", i, n.script_hash, n.event_name);
            }
        }

        tx_states = tx_engine
            .take_state::<LedgerTransactionStates>()
            .unwrap_or_else(|| LedgerTransactionStates::new(Vec::new()));

        assert_eq!(
            vm_state,
            neo_vm::VMState::HALT,
            "tx{} must HALT (gas={gas})",
            idx,
        );

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
    post_persist_engine.native_post_persist().expect("post persist");
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

    // Fetch expected root from C# RPC reference jsonl (if available).
    // C# expected: 0x363b80d78610a11da88dbe23ca23fd0494dbd61dd033a98d3f689f8205c0e93f (from log)
    let expected_csharp_root = UInt256::parse(
        "0x363b80d78610a11da88dbe23ca23fd0494dbd61dd033a98d3f689f8205c0e93f",
    )
    .expect("parse expected C# root");
    assert_eq!(
        new_root, expected_csharp_root,
        "block 679779 OnPersist + 2-HALT-tx + PostPersist state root must match C# v3.9.1",
    );
}
