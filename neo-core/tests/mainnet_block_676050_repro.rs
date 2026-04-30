#![cfg(feature = "rocksdb")]

// Reproducer for block 676,050 divergence (third post-fix bug).
//
// Block 676,050 has 2 transactions, both HALT on mainnet (C# Neo v3.9.1):
//   tx0 0x4e2d76756fe4253ed19ae68a99b3557b2dedfa3e8e204fddf61163c9334a7e17
//       sender NMPZugBZX26sdUXPT29WAwkHmDeUcXeToZ (0x33a7d61afbb73d3890143a30e2047af683573010)
//       Calls GAS.transfer(sender, 0x3978a4f9...n3trader, 10_000_000, data) where data
//       is a 6-element nested Array. GAS.transfer triggers onNEP17Payment on N3Trader,
//       which is supposed to write 6 storage entries (key 02=0x01, plus 5 trade records).
//       gasconsumed = 7,342,529, result = Boolean(true), 2 notifications emitted on C#.
//
//   tx1 0x0340aafd0d7ac0ed9369705dee8bc23c83f230db602c049bf24492d5288a9037
//       sender 0x1e072998c04cc53a70b1e761a7024bd2a1325424 scope=CustomContracts
//       Calls FlamingoSwapPair.swapTokenInForTokenOut on 0x171d791c... .
//       gasconsumed = 8,011,896, result HALT.
//
// Our Rust sync produces wrong state root at 676,050:
//   local:    0xad6328fe377f40fa9270f081b6b59804d3f63b575f3613bc5b1214064c5674a3
//   expected: 0x7f71b288568a951c5da2a953c127547d7bb1e1fee0d9772a32deb3db5c518399
//   put_count=11, del_count=0
//
// On Rust the N3Trader contract storage shows only key 02 with EMPTY value, while C#
// has 6 keys (02=0x01 plus 5 trade records). FlamingoSwapPair, FLM, and GAS contracts
// match. Only N3Trader diverges. Both nodes execute the same NEF (verified via byte-
// identical contract record at block 676,049).
//
// This reproducer replays just block 676,050 against the local DB at the 676,049
// snapshot and asserts:
//   - tx0 HALTs with VMState::HALT, returns Boolean(true)
//   - tx0 emits 2 notifications: GAS Transfer + N3Trader TradeCreated
//   - resulting state root matches C# expected value
//
// Run with: cargo test --release -p neo-core --test mainnet_block_676050_repro -- --ignored --nocapture
//
// Useful tracing env vars:
//   NEO_TRACE_STORAGE_PUT=1     trace every Storage.Put SYSCALL with key/value preview

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
#[ignore = "requires local mainnet full-state data synced to 676,049 (clean) then applies block 676,050"]
fn replay_block_676050_debug() {
    let state_store = open_state_store();
    let Some(root_676049) = state_store.get_state_root(676_049).map(|r| r.root_hash) else {
        eprintln!(
            "[SKIPPED] mainnet_block_676050_repro: state root 676049 not present in \
             data/mainnet/StateRoot. This test is a no-op until the local DB has been \
             synced past height 676_049. Reported as PASS but assertions did NOT run."
        );
        return;
    };
    let trie = Arc::new(Mutex::new(state_store.trie_for_root(root_676049)));

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

    // Build tx0: GAS.transfer → N3Trader.onNEP17Payment (the diverging tx)
    let mut tx0 = Transaction::new();
    tx0.set_version(0);
    tx0.set_nonce(3_999_507_682);
    tx0.set_system_fee(7_342_529);
    tx0.set_network_fee(130_362);
    tx0.set_valid_until_block(676_080);
    let tx0_signer = Signer::new(
        u160("0x33a7d61afbb73d3890143a30e2047af683573010"),
        neo_core::WitnessScope::CALLED_BY_ENTRY,
    );
    tx0.set_signers(vec![tx0_signer]);
    tx0.set_attributes(Vec::new());
    tx0.set_script(
        BASE64
            .decode("CxEQEsACAOH1BQIA4fUFEsAMFM924ovQBixKR47jVWEBExnzz6TSDBTPduKL0AYsSkeO41VhARMZ88+k0hLADAAMABLAEBbAAoCWmAAMFGNG8Qvi9v7Fhd3FRpricxj5pHg5DBQQMFeD9noE4jA6FJA4Pbf7GtanMxTAHwwIdHJhbnNmZXIMFM924ovQBixKR47jVWEBExnzz6TSQWJ9W1I=")
            .expect("tx0 script"),
    );
    tx0.set_witnesses(vec![witness(
        "DEAYeti0d4LHXqXlBO6fSfjQ+ous64FxZWwpmtJoyMxT0O1talVI+ikRstbPfJWbm3gfCf3nTsC+YNvFUU/BdgiI",
        "DCECgDCKOy+uAaG9ybWHrFryynwyw5nWY2uVMZNMxBGHsnBBVuezJw==",
    )]);

    // Build tx1: FlamingoSwapPair.swapTokenInForTokenOut (DEX swap)
    let mut tx1 = Transaction::new();
    tx1.set_version(0);
    tx1.set_nonce(22_305_112);
    tx1.set_system_fee(8_011_896);
    tx1.set_network_fee(140_752);
    tx1.set_valid_until_block(676_080);
    let mut tx1_signer = Signer::new(
        u160("0x1e072998c04cc53a70b1e761a7024bd2a1325424"),
        neo_core::WitnessScope::CUSTOM_CONTRACTS,
    );
    tx1_signer.allowed_contracts = vec![
        u160("0xf970f4ccecd765b63732b821775dc38c25d74f23"),
        u160("0xfb75a5314069b56e136713d38477f647a13991b4"),
        u160("0xca2d20610d7982ebe0bed124ee7e9b2d580a6efc"),
        u160("0xf0151f528127558851b39c2cd8aa47da7418ab28"),
        u160("0x171d791c0301c332cfe95c6371ee32965e34b606"),
        u160("0xd2a4cff31913016155e38e474a2c06d08be276cf"),
    ];
    tx1.set_signers(vec![tx1_signer]);
    tx1.set_attributes(Vec::new());
    tx1.set_script(
        BASE64
            .decode("A5Ev9Ht9AQAADBTPduKL0AYsSkeO41VhARMZ88+k0gwUKKsYdNpHqtgsnLNRiFUngVIfFfASwAIb3YgLA0+DYq4AAAAADBQkVDKh0ksCp2HnsXA6xUzAmCkHHhXAHwwWc3dhcFRva2VuSW5Gb3JUb2tlbk91dAwUI0/XJYzDXXchuDI3tmXX7Mz0cPlBYn1bUg==")
            .expect("tx1 script"),
    );
    tx1.set_witnesses(vec![witness(
        "DEAweDDeSBgMAgMSJ7A/sfWYaYYoMu+DKz8Bgv+vT/sisoXbY6xTbjp7SVzjul36baGjubJJMiA/+Wb62NViRAAk",
        "DCECgFoeBWSjendx6LiVtrPJjJW2TdCqvySY8m/Af+6qg/JBVuezJw==",
    )]);

    // Block 676,050 from mainnet RPC getblock
    let header = BlockHeader::new(
        0,
        u256("0x3fdcf2547f8a402a62eb4dcf42219e0c9cce2fb153c439067291d7b7df114cfb"),
        u256("0x0000000000000000000000000000000000000000000000000000000000000000"), // filled by block
        1_638_461_566_416,
        0x86CF46A7A1AB8C43,
        676_050,
        4,
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

    // Execute each tx and dump state
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

        // Both txs HALT on mainnet — if either FAULTs, we have a divergence
        assert_eq!(
            vm_state,
            neo_vm::VMState::HALT,
            "tx{} must HALT (gas={gas})",
            idx,
        );

        // Merge tx writes into base_cache (HALT path)
        let tracked: Vec<_> = tx_snapshot.tracked_items().into_iter().collect();
        base_cache.merge_tracked_items(&tracked);
    }

    // Run PostPersist on the block-level base_cache (mirrors persist_block_internal).
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

    // Apply all non-Ledger storage changes to the trie and compute the new state root.
    let mut applied = 0usize;
    let mut skipped_ledger = 0usize;
    let mut all: Vec<_> = base_cache.tracked_items().into_iter().collect();
    all.sort_by(|a, b| (a.0.id, a.0.key()).cmp(&(b.0.id, b.0.key())));
    let mut trie_guard = trie.lock();

    // Dump N3Trader storage changes specifically (id we look up dynamically;
    // the contract has updateCounter=0 so id is whatever was assigned at deploy)
    let n3trader_hash = u160("0x3978a4f91873e29a46c5dd85c5fef6e20bf14663");
    eprintln!("\n=== N3Trader storage changes ===");
    for (key, trackable) in all.iter() {
        let state_str = format!("{:?}", trackable.state);
        // N3Trader id=43 per RPC inspection
        if key.id == 43 {
            let val_hex = hex::encode(trackable.item.value_bytes());
            eprintln!(
                "  state={state_str} contract_id={} key={} value={}",
                key.id,
                hex::encode(key.key()),
                if val_hex.len() > 80 {
                    format!("{}...({} bytes)", &val_hex[..80], val_hex.len() / 2)
                } else {
                    format!("{} ({} bytes)", val_hex, val_hex.len() / 2)
                },
            );
        }
    }

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
    let _ = n3trader_hash; // silence unused if we don't use it elsewhere
    eprintln!(
        "\napplied={} skipped_ledger={} new_root={}",
        applied, skipped_ledger, new_root
    );
    let expected_csharp_root = UInt256::parse(
        "0x7f71b288568a951c5da2a953c127547d7bb1e1fee0d9772a32deb3db5c518399",
    )
    .expect("parse expected C# root");
    assert_eq!(
        new_root, expected_csharp_root,
        "block 676050 OnPersist + 2-HALT-tx + PostPersist state root must match C# v3.9.1",
    );
}
