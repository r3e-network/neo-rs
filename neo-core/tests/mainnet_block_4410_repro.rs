#![cfg(feature = "rocksdb")]

use neo_core::ledger::{Block, BlockHeader};
use neo_core::persistence::data_cache::{DataCache, DataCacheConfig, Trackable};
use neo_core::persistence::{i_store_provider::IStoreProvider, providers::RocksDBStoreProvider};
use neo_core::persistence::{SeekDirection, StorageConfig};
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_core::state_service::state_store::{
    SnapshotBackedStateStoreBackend, StateServiceSettings, StateStore,
};
use neo_core::UInt160;
use parking_lot::Mutex;
use std::path::Path;
use std::sync::Arc;

fn u160_from_address(addr: &str) -> UInt160 {
    UInt160::from_address(addr).expect("address")
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

fn block_4410() -> Block {
    let mut header = BlockHeader::default();
    header.index = 4410;
    header.timestamp = 1_627_972_036_949;
    header.primary_index = 6;
    header.next_consensus = u160_from_address("NVg7LjGcUSrgxgjX3zEgqaksfMaiS8Z6e1");
    Block::new(header, Vec::new())
}

fn tracked_suffixes(cache: &DataCache, contract_id: i32, prefix: u8) -> Vec<(Vec<u8>, Trackable)> {
    let mut items: Vec<_> = cache
        .tracked_items()
        .into_iter()
        .filter(|(key, _)| key.id == contract_id && key.key().first().copied() == Some(prefix))
        .map(|(key, trackable)| (key.suffix().to_vec(), trackable))
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));
    items
}

#[test]
#[ignore = "requires local mainnet full-state data under ./data/mainnet/StateRoot"]
fn replay_block_4410_onpersist_postpersist() {
    let state_store = open_state_store();
    let root_4409 = state_store
        .get_state_root(4409)
        .expect("state root 4409 present")
        .root_hash;
    let trie = Arc::new(Mutex::new(state_store.trie_for_root(root_4409)));

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
            move |prefix: Option<&neo_core::smart_contract::StorageKey>, _direction: SeekDirection| {
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

    let block = Arc::new(block_4410());
    let settings = ProtocolSettings::mainnet();

    let mut on_persist_engine = ApplicationEngine::new_with_shared_block(
        TriggerType::OnPersist,
        None,
        Arc::clone(&base_cache),
        Some(Arc::clone(&block)),
        settings.clone(),
        neo_core::smart_contract::application_engine::TEST_MODE_GAS,
        None,
    )
    .expect("on persist engine");
    on_persist_engine.native_on_persist().expect("on persist");

    // Replicate persist_block_internal: carry native contracts and cache forward.
    let seeded_contracts = on_persist_engine.contracts_snapshot();
    let seeded_native_cache = on_persist_engine.native_contract_cache_handle();
    let tx_states_carry = on_persist_engine
        .take_state::<neo_core::smart_contract::native::LedgerTransactionStates>()
        .unwrap_or_else(|| {
            neo_core::smart_contract::native::LedgerTransactionStates::new(Vec::new())
        });
    drop(on_persist_engine);

    let mut post_persist_engine = ApplicationEngine::new_with_preloaded_native(
        TriggerType::PostPersist,
        None,
        Arc::clone(&base_cache),
        Some(Arc::clone(&block)),
        settings,
        neo_core::smart_contract::application_engine::TEST_MODE_GAS,
        seeded_contracts,
        seeded_native_cache,
        None,
    )
    .expect("post persist engine");
    post_persist_engine.set_state(tx_states_carry);
    post_persist_engine
        .native_post_persist()
        .expect("post persist");

    eprintln!("committee writes:");
    for (suffix, trackable) in tracked_suffixes(base_cache.as_ref(), -5, 0x0e) {
        eprintln!(
            "  key={} state={:?} value={}",
            hex::encode(&suffix),
            trackable.state,
            hex::encode(trackable.item.value_bytes())
        );
    }

    eprintln!("voter reward writes:");
    for (suffix, trackable) in tracked_suffixes(base_cache.as_ref(), -5, 0x17) {
        eprintln!(
            "  key={} state={:?} value={}",
            hex::encode(&suffix),
            trackable.state,
            hex::encode(trackable.item.value_bytes())
        );
    }

    eprintln!("gas writes:");
    for (suffix, trackable) in tracked_suffixes(base_cache.as_ref(), -6, 0x14) {
        eprintln!(
            "  key={} state={:?} value={}",
            hex::encode(&suffix),
            trackable.state,
            hex::encode(trackable.item.value_bytes())
        );
    }

    eprintln!("ALL writes (non-None state):");
    let mut all: Vec<_> = base_cache.tracked_items().into_iter().collect();
    all.sort_by(|a, b| (a.0.id, a.0.key()).cmp(&(b.0.id, b.0.key())));
    for (key, trackable) in all.iter() {
        let state_str = format!("{:?}", trackable.state);
        if state_str == "None" || state_str == "NotFound" {
            continue;
        }
        eprintln!(
            "  id={} key={} state={:?} value={}",
            key.id,
            hex::encode(key.key()),
            trackable.state,
            hex::encode(trackable.item.value_bytes())
        );
    }

    // Apply all non-Ledger changes to the trie and compute new root
    eprintln!("\n=== Applying changes to trie and computing root ===");
    let mut applied = 0usize;
    let mut skipped = 0usize;
    let mut trie_guard = trie.lock();
    for (key, trackable) in all.iter() {
        let state_str = format!("{:?}", trackable.state);
        if state_str == "None" || state_str == "NotFound" {
            continue;
        }
        if key.id == -4 {
            skipped += 1;
            continue; // LedgerContract excluded
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
    eprintln!("applied={} skipped_ledger={} new_root={}", applied, skipped, new_root);
    eprintln!("expected C# root at 4410: 0x750c662a633387c5dece78ba7f71384cc10427089c9e7321e19841c93efb9b81");
}
