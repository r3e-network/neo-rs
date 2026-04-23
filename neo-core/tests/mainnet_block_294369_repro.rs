#![cfg(feature = "rocksdb")]

// Reproducer for block 294,369 divergence.
//
// Block 294,369 has 2 transactions, both FAULT on mainnet:
//   tx1 0xfdeae91c7963e8be969116bf2c7dfd99a60b05e9f2164f0418e08dfab5c8ad81
//       sender NRXVSJJ17zMJfox4FttA2G7PRZnk6Ka3kT (0x655efe92db0406b9246341881d45fa878b70903d)
//       FAULT at instruction 106 (THROW): "EndSaleInternal NEP17 transfer failed"
//       calls GhostMarket (0xcc638d55d99fc81295daccbaf722b84f179fb9c4) "bidToken"
//       1 GAS Transfer notification before fault
//
//   tx2 0xe179950c1151c9ca93f32894ff3c04224d7daba2327f02661a90fa0c5747db49
//       sender NcwZ2DLFxpmUR7UL4uhKPsaSCtf7WHNBoM (0x8de346448f3b7044d7aaf11ab6bd06bc78ccc6ba)
//       FAULT at instruction 1923 (SYSCALL System.Runtime.Notify): insufficient gas
//       calls Neoverse NFT (0xcd10d9f697230b04d9ebb8594a1ffe18fa95d9ad) "listToken"
//       2 notifications before fault (DebugEvent + Transfer "Fragment E #1380")
//
// Our sync produces wrong state root at 294,369:
//   local:    0x9bb5d06bd3d486b1ac9f05d7b94a1208ec9f8239b3f6188154e271d8741d3dfa
//   expected: 0x14de672b83fd13e8296edc7badf3e33a221eb5eb06d62e16f46966613bfa0b49
//   put_count=11, del_count=1
// This introduces a 729-block cascade through 295,097 that blocks sync at 295,098.

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
#[ignore = "requires local mainnet full-state data synced to 294,368 (clean) then applies block 294,369"]
fn replay_block_294369_debug() {
    let state_store = open_state_store();
    let Some(root_294368) = state_store.get_state_root(294_368).map(|r| r.root_hash) else {
        eprintln!("state root 294368 not present; sync not yet there");
        return;
    };
    let trie = Arc::new(Mutex::new(state_store.trie_for_root(root_294368)));

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

    // Build tx1: bidToken on GhostMarket
    let mut tx1 = Transaction::new();
    tx1.set_version(0);
    tx1.set_nonce(3_958_463_389);
    tx1.set_system_fee(8_500_825);
    tx1.set_network_fee(127_462);
    tx1.set_valid_until_block(294_399);
    let mut tx1_signer = Signer::new(
        u160("0x655efe92db0406b9246341881d45fa878b70903d"),
        neo_core::WitnessScope::CUSTOM_CONTRACTS,
    );
    tx1_signer.allowed_contracts = vec![
        u160("0xcc638d55d99fc81295daccbaf722b84f179fb9c4"),
        u160("0xcd10d9f697230b04d9ebb8594a1ffe18fa95d9ad"),
        u160("0xd2a4cff31913016155e38e474a2c06d08be276cf"),
    ];
    tx1.set_signers(vec![tx1_signer]);
    tx1.set_attributes(Vec::new());
    tx1.set_script(
        BASE64
            .decode("AgDvHA0MAvYHDBQ9kHCLh/pFHYhBYyS5BgTbkv5eZRPAHwwIYmlkVG9rZW4MFMS5nxdPuCL3uszalRLIn9lVjWPMQWJ9W1I=")
            .expect("tx1 script"),
    );
    tx1.set_witnesses(vec![witness(
        "DEDu4vaAX3uJ/zDMGTd098L+cQ18Fm/44LCt4SAenlniwJ6eT3PCC17RxKFtCOC+GFiw4MIFCIGsJ/CabqMSDbcu",
        "DCECgH+8wXUMh9pCLrX7byk9HS+7XwWFRgoQxMhUXSliOylBVuezJw==",
    )]);

    // Build tx2: listToken on Neoverse (calls GhostMarket)
    let mut tx2 = Transaction::new();
    tx2.set_version(0);
    tx2.set_nonce(2_957_850_199);
    tx2.set_system_fee(7_871_080);
    tx2.set_network_fee(133_552);
    tx2.set_valid_until_block(294_398);
    let mut tx2_signer = Signer::new(
        u160("0x8de346448f3b7044d7aaf11ab6bd06bc78ccc6ba"),
        neo_core::WitnessScope::CUSTOM_CONTRACTS,
    );
    tx2_signer.allowed_contracts = vec![
        u160("0xcc638d55d99fc81295daccbaf722b84f179fb9c4"),
        u160("0xcd10d9f697230b04d9ebb8594a1ffe18fa95d9ad"),
    ];
    tx2.set_signers(vec![tx2_signer]);
    tx2.set_attributes(Vec::new());
    tx2.set_script(
        BASE64
            .decode("EgFYAgNdUAKyfAEAAANdiIMXfAEAABACAKPhEQwQRnJhZ21lbnQgRSAjMTM4MAwUz3bii9AGLEpHjuNVYQETGfPPpNIMFLrGzHi8Br22GvGq10RwO49ERuONDBSt2ZX6GP4fSlm469kECyOX9tkQzRrAHwwJbGlzdFRva2VuDBTEuZ8XT7gi97rM2pUSyJ/ZVY1jzEFifVtS")
            .expect("tx2 script"),
    );
    tx2.set_witnesses(vec![witness(
        "DEBw078K6tBlOBaPuu2RyJkKIn92DzwVq1DSV51ARu9X5CC3w4sYOHbX3X+st7zbURZLGRAulRDMPOJ9Pophqtu5",
        "DCECkf4LoKb28yA3juQibtUAP7tL5KwqnVW1j1gPGY2b82JBVuezJw==",
    )]);

    // Block 294,369 from mainnet RPC getblock
    let header = BlockHeader::new(
        0,
        u256("0x0bfb4028cce5f3914d103e4cc38370baf38cdf0895be5dec2dc51321b565c650"),
        u256("0x0000000000000000000000000000000000000000000000000000000000000000"), // filled by block
        1_632_482_072_785,
        0x3D4B003C3B9E67D7,
        294_369,
        5,
        UInt160::from_address("NSiVJYZej4XsxG5CUpdwn7VRQk8iiiDMPM").expect("nextconsensus"),
        vec![],
    );
    let block = Arc::new(Block::new(header, vec![tx1.clone(), tx2.clone()]));

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
            LedgerTransactionStates::new(vec![
                PersistedTransactionState::new(&tx1, block.index()),
                PersistedTransactionState::new(&tx2, block.index()),
            ])
        });
    let _ = tx_states;

    // Execute each tx and dump state
    for (idx, tx) in [&tx1, &tx2].iter().enumerate() {
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
        tx_engine
            .load_script(tx.script().to_vec(), CallFlags::ALL, None)
            .expect("load");

        let vm_state = tx_engine.execute_allow_fault();
        let gas = tx_engine.gas_consumed();
        let exception = tx_engine.fault_exception();
        let notifs = tx_engine.notifications();
        eprintln!(
            "\n=== tx{} result ===\nvm_state={vm_state:?} gas={gas} exception={exception:?}",
            idx + 1,
        );
        eprintln!("notifications ({}):", notifs.len());
        for (i, n) in notifs.iter().enumerate() {
            eprintln!("  [{}] contract={} event={}", i, n.script_hash, n.event_name);
        }

        // Both txs must FAULT on mainnet — if either HALTs, gas metering is wrong
        // and the resulting state writes would cascade through subsequent blocks.
        assert_eq!(
            vm_state,
            neo_vm::VMState::FAULT,
            "tx{} must FAULT (gas={gas}) — HALT means gas is under-counted",
            idx + 1,
        );
    }
}
