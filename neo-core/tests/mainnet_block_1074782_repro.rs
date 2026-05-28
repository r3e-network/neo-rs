#![cfg(feature = "rocksdb")]

// Reproducer for block 1,074,782 — Bug #9 (first state-root divergence after the
// bug #8 fix). Bisect of state roots vs C# RPC `getstateroot` showed:
//   block 1,074,781: MATCH
//   block 1,074,782: DIFFER
//
// Block 1,074,782 has 3 transactions on mainnet (C# Neo v3.9.1):
//   tx[0] 0x9db2325b... — small CustomContracts call (sysfee 26M)
//   tx[1] 0x973d3fd3... — small CustomContracts call (sysfee 6.77M)
//   tx[2] 0x5771381f8ef72bf2fef7b9232b94596da41eda30b536ec7675f6fa8d7817b084
//         — sysfee 1,838,595,609 (18.4 GAS), HALT in C#, gasconsumed 1,820,391,692
//         — Deploys contract 0x8ecea6e434ec35f2e5ed2e4efd211cf3272fb0d2 (ValeNNTine, NEP-11)
//         — Generates 1 Deploy event + 320 Transfer events from the new contract's _deploy
//
// C# state root @ 1,074,782: 0xe80f656842033904bb2e69106765633e190ee5bbe733655c6eda13c3478d6c38
//
// This reproducer replays just block 1,074,782 against the local DB at the
// 1,074,781 snapshot and asserts the resulting state root equals C#.

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

const TX0_SCRIPT: &str = include_str!("mainnet_block_1074782_data/tx0_script.b64");
const TX0_INV: &str = include_str!("mainnet_block_1074782_data/tx0_inv.b64");
const TX0_VER: &str = include_str!("mainnet_block_1074782_data/tx0_ver.b64");
const TX1_SCRIPT: &str = include_str!("mainnet_block_1074782_data/tx1_script.b64");
const TX1_INV: &str = include_str!("mainnet_block_1074782_data/tx1_inv.b64");
const TX1_VER: &str = include_str!("mainnet_block_1074782_data/tx1_ver.b64");
const TX2_SCRIPT: &str = include_str!("mainnet_block_1074782_data/tx2_script.b64");
const TX2_INV: &str = include_str!("mainnet_block_1074782_data/tx2_inv.b64");
const TX2_VER: &str = include_str!("mainnet_block_1074782_data/tx2_ver.b64");

#[test]
#[ignore = "requires local mainnet full-state data synced past height 1,074,781"]
fn replay_block_1074782_debug() {
    let state_store = open_state_store();
    let Some(root_prev) = state_store.get_state_root(1_074_781).map(|r| r.root_hash) else {
        eprintln!(
            "[SKIPPED] mainnet_block_1074782_repro: state root 1074781 not present in \
             data/mainnet/StateRoot. This test is a no-op until the local DB has been \
             synced past height 1_074_781."
        );
        return;
    };
    eprintln!("loaded prior state root @1074781 = {}", root_prev);
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

    // tx[0] — small CustomContracts call
    let mut tx0 = Transaction::new();
    tx0.set_version(0);
    tx0.set_nonce(3_008_078_233);
    tx0.set_system_fee(26_000_000);
    tx0.set_network_fee(200_000);
    tx0.set_valid_until_block(1_075_782);
    let mut tx0_signer = Signer::new(
        u160("0xa0fd0960399813001baa6ac8b84ad6801b321bc7"),
        neo_core::WitnessScope::CUSTOM_CONTRACTS,
    );
    tx0_signer.allowed_contracts = vec![
        u160("0xf970f4ccecd765b63732b821775dc38c25d74f23"),
        u160("0xfb75a5314069b56e136713d38477f647a13991b4"),
        u160("0xca2d20610d7982ebe0bed124ee7e9b2d580a6efc"),
        u160("0x48c40d4666f93408be1bef038b6722404d9a4c2a"),
        u160("0xcd48b160c1bbc9d74997b803b9a7ad50a4bef020"),
        u160("0x545dee8354823d1bdf4ac524e4092f7405025247"),
    ];
    tx0.set_signers(vec![tx0_signer]);
    tx0.set_attributes(Vec::new());
    tx0.set_script(BASE64.decode(TX0_SCRIPT.trim()).expect("tx0 script"));
    tx0.set_witnesses(vec![witness(TX0_INV.trim(), TX0_VER.trim())]);

    // tx[1] — small CustomContracts call
    let mut tx1 = Transaction::new();
    tx1.set_version(0);
    tx1.set_nonce(4_138_530_943);
    tx1.set_system_fee(6_771_429);
    tx1.set_network_fee(132_262);
    tx1.set_valid_until_block(1_074_812);
    let mut tx1_signer = Signer::new(
        u160("0x760f19379755a91c11396441ec055b954042809f"),
        neo_core::WitnessScope::CUSTOM_CONTRACTS,
    );
    tx1_signer.allowed_contracts = vec![
        u160("0xcc638d55d99fc81295daccbaf722b84f179fb9c4"),
        u160("0x577a51f7d39162c9de1db12a6b319c848e4c54e5"),
    ];
    tx1.set_signers(vec![tx1_signer]);
    tx1.set_attributes(Vec::new());
    tx1.set_script(BASE64.decode(TX1_SCRIPT.trim()).expect("tx1 script"));
    tx1.set_witnesses(vec![witness(TX1_INV.trim(), TX1_VER.trim())]);

    // tx[2] — ValeNNTine NFT deploy + 320-mint storm (the suspected divergence source)
    let mut tx2 = Transaction::new();
    tx2.set_version(0);
    tx2.set_nonce(2_636_222_762);
    tx2.set_system_fee(1_838_595_609);
    tx2.set_network_fee(2_335_562);
    tx2.set_valid_until_block(1_074_812);
    let tx2_signer = Signer::new(
        u160("0x32691ec1229514e6941b53353f97cbbc462cfc1d"),
        neo_core::WitnessScope::CALLED_BY_ENTRY,
    );
    tx2.set_signers(vec![tx2_signer]);
    tx2.set_attributes(Vec::new());
    tx2.set_script(BASE64.decode(TX2_SCRIPT.trim()).expect("tx2 script"));
    tx2.set_witnesses(vec![witness(TX2_INV.trim(), TX2_VER.trim())]);

    let header = BlockHeader::new(
        0,
        u256("0x11002b37048d3c6c8745214dcf9a0809d9d399e71d74d08aa7ca830cc8940f65"),
        u256("0x0000000000000000000000000000000000000000000000000000000000000000"),
        1_644_863_126_320,
        0x067B109059597128,
        1_074_782,
        2,
        UInt160::from_address("NSiVJYZej4XsxG5CUpdwn7VRQk8iiiDMPM").expect("nextconsensus"),
        vec![],
    );
    let block = Arc::new(Block::new(
        header,
        vec![tx0.clone(), tx1.clone(), tx2.clone()],
    ));

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
                PersistedTransactionState::new(&tx2, block.index()),
            ])
        });
    drop(on_persist_engine);

    for (idx, tx) in [&tx0, &tx1, &tx2].iter().enumerate() {
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
            for (i, n) in notifs.iter().take(20).enumerate() {
                eprintln!(
                    "  [{}] contract={} event={}",
                    i, n.script_hash, n.event_name
                );
            }
            if notifs.len() > 20 {
                eprintln!("  ... +{} more notifications", notifs.len() - 20);
            }
        }

        tx_states = tx_engine
            .take_state::<LedgerTransactionStates>()
            .unwrap_or_else(|| LedgerTransactionStates::new(Vec::new()));

        // tx2 is expected to HALT (gasconsumed=1820391692 in C# applog).
        if vm_state != neo_vm_rs::VmState::HALT {
            eprintln!(
                "\n!!! tx{} did not HALT — bug #9 reproduced (gas={gas}) !!!",
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

    // Dump all tracked storage writes to /tmp/blk1074782_rust_storage.txt for diff vs C#.
    if let Ok(out_path) = std::env::var("NEO_DUMP_STORAGE_PATH") {
        use std::io::Write;
        let mut f = std::fs::File::create(&out_path).expect("open dump file");
        for (key, trackable) in all.iter() {
            let state_str = format!("{:?}", trackable.state);
            if state_str == "None" || state_str == "NotFound" {
                continue;
            }
            if key.id == -4 {
                continue;
            }
            let key_bytes = key.to_array();
            let value_bytes: Vec<u8> = match state_str.as_str() {
                "Added" | "Changed" => trackable.item.value_bytes().to_vec(),
                "Deleted" => Vec::new(),
                _ => continue,
            };
            writeln!(
                f,
                "{} id={} keyhex={} valhex={}",
                state_str,
                key.id,
                hex::encode(&key_bytes),
                hex::encode(&value_bytes),
            )
            .expect("write");
        }
        eprintln!("storage writes dumped to {}", out_path);
    }

    // C# state root @ 1,074,782 (from mainnet1.neo.coz.io:443 getstateroot).
    let expected_csharp_root =
        UInt256::parse("0xe80f656842033904bb2e69106765633e190ee5bbe733655c6eda13c3478d6c38")
            .expect("parse expected C# root");
    assert_eq!(
        new_root, expected_csharp_root,
        "block 1,074,782 OnPersist + 3-tx + PostPersist state root must match C# v3.9.1",
    );
}
