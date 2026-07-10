use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use neo_blockchain::SystemContext;
use neo_config::ProtocolSettings;
use neo_native_contracts::StandardNativeProvider;
use neo_storage::persistence::StoreCache;
use neo_storage::persistence::providers::MemoryStore;
use neo_storage::{StorageItem, StorageKey};

use crate::{BlockCommitHooks, NodeSystemContext, NoopBlockCommitHooks};

#[derive(Debug, Default)]
struct RecordingCommitHooks {
    fenced: AtomicBool,
    succeeded: AtomicBool,
    failed: AtomicBool,
}

impl<B> BlockCommitHooks<B> for RecordingCommitHooks
where
    B: neo_storage::CacheRead,
{
    fn fence_precommit_durability(&self) -> Result<(), String> {
        self.fenced.store(true, Ordering::Release);
        Ok(())
    }

    fn canonical_commit_succeeded(&self) {
        self.succeeded.store(true, Ordering::Release);
    }

    fn canonical_commit_failed(&self, _reason: &str) {
        self.failed.store(true, Ordering::Release);
    }
}

#[derive(Debug, Default)]
struct FailingFenceHooks;

impl<B> BlockCommitHooks<B> for FailingFenceHooks
where
    B: neo_storage::CacheRead,
{
    fn fence_precommit_durability(&self) -> Result<(), String> {
        Err("injected observer flush failure".to_string())
    }
}

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

    context
        .commit_to_store()
        .expect("commit canonical snapshot");

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

#[test]
fn durable_commit_outcome_is_reported_to_application_hooks() {
    let store = Arc::new(MemoryStore::new());
    let writable_cache = StoreCache::new_from_store(Arc::clone(&store), false);
    let writable_snapshot = Arc::new(writable_cache.data_cache().clone());
    let success_hooks = Arc::new(RecordingCommitHooks::default());
    let writable_context = NodeSystemContext::new(
        Arc::new(ProtocolSettings::default()),
        writable_snapshot,
        writable_cache,
        Arc::new(StandardNativeProvider::new()),
        Arc::clone(&success_hooks),
    );

    writable_context
        .commit_to_store()
        .expect("writable canonical store should commit");
    assert!(success_hooks.fenced.load(Ordering::Acquire));
    assert!(success_hooks.succeeded.load(Ordering::Acquire));
    assert!(!success_hooks.failed.load(Ordering::Acquire));
    assert!(!writable_context.should_stop_blockchain_service());

    let read_only_cache = StoreCache::new_from_store(store, true);
    let read_only_snapshot = Arc::new(read_only_cache.data_cache().clone());
    let failure_hooks = Arc::new(RecordingCommitHooks::default());
    let read_only_context = NodeSystemContext::new(
        Arc::new(ProtocolSettings::default()),
        read_only_snapshot,
        read_only_cache,
        Arc::new(StandardNativeProvider::new()),
        Arc::clone(&failure_hooks),
    );

    read_only_context
        .commit_to_store()
        .expect_err("read-only canonical store must reject a durability fence");
    assert!(!failure_hooks.succeeded.load(Ordering::Acquire));
    assert!(failure_hooks.failed.load(Ordering::Acquire));
    assert!(read_only_context.should_stop_blockchain_service());
}

#[test]
fn observer_durability_failure_prevents_canonical_store_commit() {
    let store = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(Arc::clone(&store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    let key = StorageKey::new(-1, vec![0xFE]);
    snapshot.add(key.clone(), StorageItem::from_bytes(vec![0x01]));
    let context = NodeSystemContext::new(
        Arc::new(ProtocolSettings::default()),
        Arc::clone(&snapshot),
        store_cache,
        Arc::new(StandardNativeProvider::new()),
        Arc::new(FailingFenceHooks),
    );

    let error = context
        .commit_to_store()
        .expect_err("observer durability failure must block canonical commit");

    assert!(error.contains("injected observer flush failure"));
    assert!(context.should_stop_blockchain_service());
    assert_eq!(snapshot.pending_change_count(), 0);
    let reader = StoreCache::new_from_store(store, false);
    assert!(reader.data_cache().get(&key).is_none());
}
