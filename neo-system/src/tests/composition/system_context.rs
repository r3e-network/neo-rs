use std::sync::Arc;

use neo_blockchain::SystemContext;
use neo_config::ProtocolSettings;
use neo_native_contracts::StandardNativeProvider;
use neo_storage::persistence::StoreCache;
use neo_storage::persistence::providers::MemoryStore;
use neo_storage::{StorageItem, StorageKey};

use crate::{NodeSystemContext, NoopBlockCommitHooks};

#[test]
fn commit_to_store_flushes_the_canonical_snapshot() {
    let store = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(Arc::clone(&store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    let context = NodeSystemContext::new(
        Arc::new(ProtocolSettings::default()),
        Arc::clone(&snapshot),
        store_cache,
        Arc::new(StandardNativeProvider::new()),
        Arc::new(NoopBlockCommitHooks),
    );
    let key = StorageKey::new(-1, vec![0xAB, 0xCD]);
    snapshot.add(key.clone(), StorageItem::from_bytes(vec![0x01, 0x02, 0x03]));

    let before = StoreCache::new_from_store(Arc::clone(&store), false);
    assert!(before.data_cache().get(&key).is_none());

    context.commit_to_store();

    let after = StoreCache::new_from_store(store, false);
    assert_eq!(
        after.data_cache().get(&key).map(|item| item.to_value()),
        Some(vec![0x01, 0x02, 0x03])
    );
}

#[test]
fn context_exposes_the_composed_native_provider() {
    let store = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(store, false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    let provider = Arc::new(StandardNativeProvider::new());
    let context = NodeSystemContext::new(
        Arc::new(ProtocolSettings::default()),
        snapshot,
        store_cache,
        Arc::clone(&provider),
        Arc::new(NoopBlockCommitHooks),
    );

    let exposed = context
        .native_contract_provider()
        .expect("composed context always exposes its native provider");
    assert!(Arc::ptr_eq(&provider, &exposed));
}
