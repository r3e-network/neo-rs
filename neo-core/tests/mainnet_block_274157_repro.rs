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
        BASE64.decode(verification_b64).expect("base64 verification"),
    )
}

fn open_state_store() -> StateStore {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .to_path_buf();
    // Set NEO_REPRO_DB_PATH to point at a richer backup that has block 274,156
    // (e.g. data/mainnet.pre-274157-fix-20260419/StateRoot).
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
#[ignore = "requires local mainnet full-state data synced past 274157"]
fn replay_block_274157_debug() {
    // tx hash 0x27d8e5db1db58b5110337ed34241782505f05653dc56c55eb3d7e7af4041d7e9
    // This tx calls BurgerNEO (0x48c40d46...).transfer(from=user, to=self, amount=1, data=null)
    // In C# HALTs; in ours FAULTs (hypothesis).

    let state_store = open_state_store();
    let Some(root_274156) = state_store.get_state_root(274_156).map(|r| r.root_hash) else {
        eprintln!(
            "[SKIPPED] mainnet_block_274157_repro: state root 274156 not present. \
             Set NEO_REPRO_DB_PATH=data/mainnet.pre-274157-fix-20260419/StateRoot \
             to point at a backup with block 274,156 (and exercise the C# state-root \
             assertion). Reported as PASS but assertions did NOT run."
        );
        return;
    };
    let trie = Arc::new(Mutex::new(state_store.trie_for_root(root_274156)));

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

    // Build block 274,157 with its single tx.
    let signer = Signer::new(
        u160("0x3ebaddf7f9cb54487b8918c9e3bde88d226baa20"),
        neo_core::WitnessScope::CALLED_BY_ENTRY,
    );

    let mut tx = Transaction::new();
    tx.set_version(0);
    // Values from mainnet RPC getblock(274157) — using exact values matters because
    // OnPersist burns (system_fee + network_fee) from sender and mints network_fee
    // to primary validator, so any discrepancy directly affects final GAS balances.
    tx.set_nonce(2_531_694_446);
    tx.set_system_fee(6_644_246);
    tx.set_network_fee(122_752);
    tx.set_valid_until_block(274_396);
    tx.set_signers(vec![signer]);
    tx.set_attributes(Vec::new());
    // script: transfer 1 BurgerNEO from user to contract (self)
    tx.set_script(
        BASE64
            .decode("CxEMFCpMmk1AImeLA+8bvgg0+WZGDcRIDBQgqmsijei948kYiXtIVMv59926PhTAHwwIdHJhbnNmZXIMFCpMmk1AImeLA+8bvgg0+WZGDcRIQWJ9W1I=")
            .expect("script"),
    );
    tx.set_witnesses(vec![witness(
        // Placeholder witness; local replay doesn't validate signatures by default.
        "DEC4MD5wI8VTiWbHOlNRvIztBYdB7EPVzxOlVHbLYT+j/EFtcxyXfnDAIOTqeM0s0SRprMf5Kpo4/Drp5B+m3y5/",
        "DCECcCBOlRTJboIjs+ZDa3mu4PKIn1OLPDZV1LtmUwUf5pxBVuezJw==",
    )]);

    let header = BlockHeader::new(
        0,
        u256("0x42f6d000000000000000000000000000000000000000000000000000000042f6"),
        u256("0x27d8e5db1db58b5110337ed34241782505f05653dc56c55eb3d7e7af4041d7e9"),
        1_631_900_000_000,
        0,
        274_157,
        2,
        UInt160::from_address("NSiVJYZej4XsxG5CUpdwn7VRQk8iiiDMPM").expect("nextconsensus"),
        vec![],
    );
    let block = Arc::new(Block::new(header, vec![tx.clone()]));

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

    // Debug: dump BurgerNEO contract script as loaded from the 274,156 trie
    let burger_hash = u160("0x48c40d4666f93408be1bef038b6722404d9a4c2a");
    if let Ok(Some(contract)) =
        neo_core::smart_contract::native::ContractManagement::get_contract_from_snapshot(
            tx_snapshot.as_ref(),
            &burger_hash,
        )
    {
        eprintln!(
            "BurgerNEO at 274,156: id={} update_counter={} script_len={}",
            contract.id,
            contract.update_counter,
            contract.nef.script.len(),
        );
        for m in &contract.manifest.abi.methods {
            eprintln!(
                "  method: name={} offset={} params={} return_type={:?}",
                m.name, m.offset, m.parameters.len(), m.return_type
            );
        }
    } else {
        eprintln!("BurgerNEO not found in snapshot at 274,156");
    }

    tx_engine
        .load_script(tx.script().to_vec(), CallFlags::ALL, None)
        .expect("load");

    let vm_state = tx_engine.execute_allow_fault();
    eprintln!(
        "vm_state={vm_state:?} gas={} exception={:?}",
        tx_engine.gas_consumed(),
        tx_engine.fault_exception()
    );

    eprintln!("result_stack:");
    for (i, item) in tx_engine.result_stack().iter().enumerate() {
        eprintln!("  [{}] {:?}", i, item);
    }

    eprintln!("notifications ({}):", tx_engine.notifications().len());
    for (i, n) in tx_engine.notifications().iter().enumerate() {
        eprintln!("  [{}] contract={} event={}", i, n.script_hash, n.event_name);
        for s in &n.state {
            eprintln!("      {:?}", s);
        }
    }

    // Dump tx_snapshot tracked items (primary storage changes)
    let mut tx_writes = 0;
    eprintln!("\ntx writes (from tx_snapshot):");
    for (key, trackable) in tx_snapshot.tracked_items() {
        let state_str = format!("{:?}", trackable.state);
        if state_str == "None" || state_str == "NotFound" { continue; }
        tx_writes += 1;
        eprintln!(
            "  id={} key={} state={:?} val={}",
            key.id,
            hex::encode(key.key()),
            trackable.state,
            hex::encode(trackable.item.value_bytes())
        );
    }
    eprintln!("total tx_snapshot writes: {}", tx_writes);

    // Dump base_cache tracked items (may include merged nested writes)
    let mut base_writes = 0;
    eprintln!("\nbase_cache tracked items (after tx):");
    for (key, trackable) in base_cache.tracked_items() {
        let state_str = format!("{:?}", trackable.state);
        if state_str == "None" || state_str == "NotFound" { continue; }
        base_writes += 1;
        eprintln!(
            "  id={} key={} state={:?} val={}",
            key.id,
            hex::encode(key.key()),
            trackable.state,
            hex::encode(trackable.item.value_bytes())
        );
    }
    eprintln!("total base_cache writes: {}", base_writes);

    // Mirror persist_block_internal: HALT merges tx_snapshot writes into
    // base_cache; FAULT discards them. The earlier `tx_snapshot.commit()` was
    // a no-op because tx_snapshot has no commit_apply wired.
    if matches!(vm_state, neo_vm::VMState::HALT) {
        let tracked = tx_snapshot.tracked_items();
        base_cache.merge_tracked_items(&tracked);
    }
    let mut post_commit_writes = 0;
    eprintln!("\nbase_cache tracked items (after merge):");
    for (key, trackable) in base_cache.tracked_items() {
        let state_str = format!("{:?}", trackable.state);
        if state_str == "None" || state_str == "NotFound" { continue; }
        post_commit_writes += 1;
        eprintln!(
            "  id={} key={} state={:?} val={}",
            key.id,
            hex::encode(key.key()),
            trackable.state,
            hex::encode(trackable.item.value_bytes())
        );
    }
    eprintln!("total base_cache writes after merge: {}", post_commit_writes);

    // Take ledger tx_states from tx_engine before PostPersist needs them.
    let tx_states_after = tx_engine
        .take_state::<LedgerTransactionStates>()
        .unwrap_or_else(|| LedgerTransactionStates::new(Vec::new()));
    drop(tx_engine);

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
    post_persist_engine.set_state(tx_states_after);
    post_persist_engine.native_post_persist().expect("post persist");
    drop(post_persist_engine);

    // Apply non-Ledger storage changes to trie and assert root matches C# v3.9.1.
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
        "0xb757b3a61363c6f851d035048fd77b03254e712ae91728189a5d5bdb1e306a5d",
    )
    .expect("parse expected C# root");
    assert_eq!(
        new_root, expected_csharp_root,
        "block 274157 OnPersist + BurgerNEO-FAULT + PostPersist state root must match C# v3.9.1",
    );
}
