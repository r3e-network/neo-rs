use super::*;
use crate::StorageError;
use crate::persistence::providers::memory_store::MemoryStore;
use crate::persistence::read_only_store::{RawReadOnlyStore, ReadOnlyStore, ReadOnlyStoreGeneric};
use crate::persistence::seek_direction::SeekDirection;
use crate::persistence::store::Store;
use crate::persistence::write_store::WriteStore;
use crate::types::{StorageItem, StorageKey};
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Clone, Copy)]
enum OverlayMode {
    Unsupported,
    Fails,
    FlushFails,
    Borrowed,
    DurableBorrowed,
    Materialized,
}

#[derive(Debug)]
struct OverlayContractStore {
    inner: MemoryStore,
    mode: OverlayMode,
    borrowed_overlay: Mutex<Vec<(Vec<u8>, Option<Vec<u8>>)>>,
    durable_overlay: Mutex<Vec<(Vec<u8>, Option<Vec<u8>>)>>,
    cloned_overlay: Mutex<Vec<(Vec<u8>, Option<Vec<u8>>)>>,
    cloned_raw_overlay_attempts: AtomicUsize,
}

impl OverlayContractStore {
    fn new(mode: OverlayMode) -> Self {
        Self {
            inner: MemoryStore::new(),
            mode,
            borrowed_overlay: Mutex::new(Vec::new()),
            durable_overlay: Mutex::new(Vec::new()),
            cloned_overlay: Mutex::new(Vec::new()),
            cloned_raw_overlay_attempts: AtomicUsize::new(0),
        }
    }

    fn borrowed_overlay(&self) -> Vec<(Vec<u8>, Option<Vec<u8>>)> {
        self.borrowed_overlay.lock().clone()
    }

    fn durable_overlay(&self) -> Vec<(Vec<u8>, Option<Vec<u8>>)> {
        self.durable_overlay.lock().clone()
    }

    fn cloned_overlay(&self) -> Vec<(Vec<u8>, Option<Vec<u8>>)> {
        self.cloned_overlay.lock().clone()
    }

    fn cloned_raw_overlay_attempts(&self) -> usize {
        self.cloned_raw_overlay_attempts.load(Ordering::Relaxed)
    }
}

impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for OverlayContractStore {
    type FindIterator<'a> =
        <MemoryStore as ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>>>::FindIterator<'a>;

    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        self.inner.try_get(key)
    }

    fn find(
        &self,
        key_prefix: Option<&Vec<u8>>,
        direction: SeekDirection,
    ) -> Self::FindIterator<'_> {
        self.inner.find(key_prefix, direction)
    }
}

impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for OverlayContractStore {
    type FindIterator<'a> =
        <MemoryStore as ReadOnlyStoreGeneric<StorageKey, StorageItem>>::FindIterator<'a>;

    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        self.inner.try_get(key)
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Self::FindIterator<'_> {
        self.inner.find(key_prefix, direction)
    }
}

impl RawReadOnlyStore for OverlayContractStore {
    fn try_get_bytes(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.inner.try_get_bytes(key)
    }
}

impl WriteStore<Vec<u8>, Vec<u8>> for OverlayContractStore {
    fn delete(&mut self, key: Vec<u8>) -> crate::StorageResult<()> {
        self.inner.delete(key)
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> crate::StorageResult<()> {
        self.inner.put(key, value)
    }
}

impl ReadOnlyStore for OverlayContractStore {}

impl Store for OverlayContractStore {
    type Snapshot = <MemoryStore as Store>::Snapshot;

    fn snapshot(&self) -> Arc<Self::Snapshot> {
        self.inner.snapshot()
    }

    fn try_commit_raw_overlay(
        &self,
        overlay: &[(Vec<u8>, Option<Vec<u8>>)],
    ) -> crate::StorageResult<bool> {
        if matches!(self.mode, OverlayMode::Unsupported) {
            return Ok(false);
        }
        self.cloned_raw_overlay_attempts
            .fetch_add(1, Ordering::Relaxed);
        match self.mode {
            OverlayMode::Unsupported => unreachable!("handled before recording overlay attempts"),
            OverlayMode::Fails => Err(StorageError::backend("raw overlay failed")),
            OverlayMode::FlushFails => {
                self.cloned_overlay.lock().extend_from_slice(overlay);
                Ok(true)
            }
            OverlayMode::Borrowed => {
                panic!("borrowed overlay mode must not materialize the cloned raw overlay")
            }
            OverlayMode::DurableBorrowed => {
                panic!("durable overlay mode must not use the ordinary raw overlay path")
            }
            OverlayMode::Materialized => {
                self.cloned_overlay.lock().extend_from_slice(overlay);
                Ok(true)
            }
        }
    }

    fn flush(&self) -> crate::StorageResult<()> {
        if matches!(self.mode, OverlayMode::FlushFails) {
            return Err(StorageError::backend("injected backend flush failure"));
        }
        Ok(())
    }

    fn try_commit_borrowed_raw_overlay<O>(
        &self,
        overlay_source: &mut O,
    ) -> crate::StorageResult<bool>
    where
        O: crate::persistence::RawOverlaySource + ?Sized,
    {
        if matches!(self.mode, OverlayMode::DurableBorrowed) {
            panic!("durable commits must not use the ordinary borrowed overlay path");
        }
        if !matches!(self.mode, OverlayMode::Borrowed) {
            return Ok(false);
        }

        let mut overlay = self.borrowed_overlay.lock();
        let mut collect = |key: &[u8], value: Option<&[u8]>| {
            overlay.push((key.to_vec(), value.map(<[u8]>::to_vec)));
        };
        overlay_source.visit_raw_overlay(&mut collect);
        Ok(true)
    }

    fn try_commit_durable_borrowed_raw_overlay<O>(
        &self,
        overlay_source: &mut O,
    ) -> crate::StorageResult<bool>
    where
        O: crate::persistence::RawOverlaySource + ?Sized,
    {
        if matches!(self.mode, OverlayMode::Fails) {
            return Err(StorageError::backend("durable overlay failed"));
        }
        if !matches!(self.mode, OverlayMode::DurableBorrowed) {
            return Ok(false);
        }

        let mut overlay = self.durable_overlay.lock();
        let mut collect = |key: &[u8], value: Option<&[u8]>| {
            overlay.push((key.to_vec(), value.map(<[u8]>::to_vec)));
        };
        overlay_source.visit_raw_overlay(&mut collect);
        Ok(true)
    }
}

#[test]
fn unsupported_raw_overlay_falls_back_to_snapshot_commit() {
    let store = Arc::new(OverlayContractStore::new(OverlayMode::Unsupported));
    let key = StorageKey::new(7, vec![0x01]);

    let mut writer = StoreCache::new_from_store(Arc::clone(&store), false);
    writer.add(key.clone(), StorageItem::from_bytes(vec![0xAA]));
    writer
        .try_commit()
        .expect("snapshot fallback should persist changes");

    assert_eq!(writer.data_cache().pending_change_count(), 0);
    assert_eq!(
        store.try_get(&key).map(|item| item.to_value()),
        Some(vec![0xAA])
    );
}

#[test]
fn snapshot_fallback_skips_raw_overlay_clone_when_store_does_not_support_it() {
    let store = Arc::new(OverlayContractStore::new(OverlayMode::Unsupported));
    let key = StorageKey::new(7, vec![0x05]);

    let mut writer = StoreCache::new_from_store(Arc::clone(&store), false);
    writer.add(key.clone(), StorageItem::from_bytes(vec![0xEE]));

    writer
        .try_commit()
        .expect("snapshot fallback should persist changes");

    assert_eq!(
        store.cloned_raw_overlay_attempts(),
        0,
        "unsupported stores should fall back to snapshots without cloning the raw overlay"
    );
    assert_eq!(
        writer.data_cache().tracked_items_call_count(),
        0,
        "snapshot fallback should visit tracked items directly instead of cloning the whole change set"
    );
    assert_eq!(
        store.try_get(&key).map(|item| item.to_value()),
        Some(vec![0xEE])
    );
}

#[test]
fn raw_overlay_error_propagates_and_keeps_cache_dirty() {
    let store = Arc::new(OverlayContractStore::new(OverlayMode::Fails));
    let key = StorageKey::new(7, vec![0x02]);

    let mut writer = StoreCache::new_from_store(Arc::clone(&store), false);
    writer.add(key.clone(), StorageItem::from_bytes(vec![0xBB]));

    let err = writer
        .try_commit()
        .expect_err("raw overlay failure should not fall back silently");

    assert!(
        err.to_string().contains("raw overlay failed"),
        "unexpected error: {err}"
    );
    assert_eq!(writer.data_cache().pending_change_count(), 1);
    assert!(
        store.try_get(&key).is_none(),
        "failed commit must not persist partial changes"
    );
}

#[test]
fn discard_pending_changes_restores_last_committed_view_after_failure() {
    let store = Arc::new(OverlayContractStore::new(OverlayMode::Fails));
    let key = StorageKey::new(7, vec![0x22]);
    let mut writer = StoreCache::new_from_store(Arc::clone(&store), false);
    writer.add(key.clone(), StorageItem::from_bytes(vec![0xBB]));

    writer
        .try_commit()
        .expect_err("injected overlay failure should reach the caller");
    writer.discard_pending_changes();

    assert_eq!(writer.data_cache().pending_change_count(), 0);
    assert!(writer.get(&key).is_none());
    assert!(store.try_get(&key).is_none());
}

#[test]
fn durable_commit_propagates_backend_flush_failure_and_clears_overlay() {
    let store = Arc::new(OverlayContractStore::new(OverlayMode::FlushFails));
    let mut writer = StoreCache::new_from_store(Arc::clone(&store), false);

    let error = writer
        .try_commit_durable()
        .expect_err("backend flush failure must reach the durability caller");

    assert!(error.to_string().contains("injected backend flush failure"));
    assert_eq!(writer.data_cache().pending_change_count(), 0);
}

#[test]
fn durable_commit_clears_overlay_when_overlay_transaction_fails() {
    let store = Arc::new(OverlayContractStore::new(OverlayMode::Fails));
    let key = StorageKey::new(7, vec![0x24]);
    let mut writer = StoreCache::new_from_store(Arc::clone(&store), false);
    writer.add(key.clone(), StorageItem::from_bytes(vec![0xDD]));

    let error = writer
        .try_commit_durable()
        .expect_err("overlay transaction failure must reach the durability caller");

    assert!(error.to_string().contains("durable overlay failed"));
    assert_eq!(writer.data_cache().pending_change_count(), 0);
    assert!(writer.get(&key).is_none());
    assert!(store.try_get(&key).is_none());
}

#[test]
fn durable_commit_rejects_backend_without_atomic_overlay_capability() {
    let store = Arc::new(OverlayContractStore::new(OverlayMode::Materialized));
    let key = StorageKey::new(7, vec![0x26]);
    let mut writer = StoreCache::new_from_store(Arc::clone(&store), false);
    writer.add(key.clone(), StorageItem::from_bytes(vec![0xEF]));

    let error = writer
        .try_commit_durable()
        .expect_err("canonical commit must require an atomic durable overlay");

    assert!(
        error.to_string().contains("atomic durable overlay"),
        "unexpected error: {error}"
    );
    assert_eq!(store.cloned_raw_overlay_attempts(), 0);
    assert_eq!(writer.data_cache().pending_change_count(), 0);
    assert!(store.try_get(&key).is_none());
}

#[test]
fn durable_commit_prefers_backend_durable_overlay_capability() {
    let store = Arc::new(OverlayContractStore::new(OverlayMode::DurableBorrowed));
    let key = StorageKey::new(7, vec![0x25]);
    let mut writer = StoreCache::new_from_store(Arc::clone(&store), false);
    writer.add(key.clone(), StorageItem::from_bytes(vec![0xEE]));

    writer
        .try_commit_durable()
        .expect("durable backend overlay should be accepted");

    assert_eq!(writer.data_cache().pending_change_count(), 0);
    assert_eq!(
        store.durable_overlay(),
        vec![(key.to_array(), Some(vec![0xEE]))]
    );
}

#[test]
fn borrowed_raw_overlay_fast_path_commits_without_cloned_overlay() {
    let store = Arc::new(OverlayContractStore::new(OverlayMode::Borrowed));
    let added = StorageKey::new(7, vec![0x03]);
    let deleted = StorageKey::new(7, vec![0x04]);

    let mut writer = StoreCache::new_from_store(Arc::clone(&store), false);
    writer.add(added.clone(), StorageItem::from_bytes(vec![0xCC]));
    writer.update(deleted.clone(), StorageItem::from_bytes(vec![0xDD]));
    writer.delete(deleted.clone());

    writer
        .try_commit()
        .expect("borrowed overlay commit should be accepted");

    assert_eq!(writer.data_cache().pending_change_count(), 0);
    let mut borrowed_overlay = store.borrowed_overlay();
    borrowed_overlay.sort_by(|left, right| left.0.cmp(&right.0));
    assert_eq!(
        borrowed_overlay,
        vec![
            (added.to_array(), Some(vec![0xCC])),
            (deleted.to_array(), None),
        ]
    );
}

#[test]
fn borrowed_raw_overlay_fast_path_emits_entries_in_key_order() {
    let store = Arc::new(OverlayContractStore::new(OverlayMode::Borrowed));
    let high_key = StorageKey::new(7, vec![0x30]);
    let low_key = StorageKey::new(7, vec![0x10]);
    let mid_key = StorageKey::new(7, vec![0x20]);

    let mut writer = StoreCache::new_from_store(Arc::clone(&store), false);
    writer.add(high_key.clone(), StorageItem::from_bytes(vec![0x03]));
    writer.add(low_key.clone(), StorageItem::from_bytes(vec![0x01]));
    writer.add(mid_key.clone(), StorageItem::from_bytes(vec![0x02]));

    writer
        .try_commit()
        .expect("borrowed overlay commit should be accepted");

    let borrowed_overlay = store.borrowed_overlay();
    assert_eq!(
        borrowed_overlay,
        vec![
            (low_key.to_array(), Some(vec![0x01])),
            (mid_key.to_array(), Some(vec![0x02])),
            (high_key.to_array(), Some(vec![0x03])),
        ],
        "backend overlay entries should be canonicalized before commit"
    );
}

#[test]
fn materialized_raw_overlay_fallback_emits_entries_in_key_order() {
    let store = Arc::new(OverlayContractStore::new(OverlayMode::Materialized));
    let high_key = StorageKey::new(7, vec![0x30]);
    let low_key = StorageKey::new(7, vec![0x10]);
    let mid_key = StorageKey::new(7, vec![0x20]);

    let mut writer = StoreCache::new_from_store(Arc::clone(&store), false);
    writer.add(high_key.clone(), StorageItem::from_bytes(vec![0x03]));
    writer.add(low_key.clone(), StorageItem::from_bytes(vec![0x01]));
    writer.add(mid_key.clone(), StorageItem::from_bytes(vec![0x02]));

    writer
        .try_commit()
        .expect("materialized overlay commit should be accepted");

    assert_eq!(
        store.cloned_overlay(),
        vec![
            (low_key.to_array(), Some(vec![0x01])),
            (mid_key.to_array(), Some(vec![0x02])),
            (high_key.to_array(), Some(vec![0x03])),
        ],
        "materialized backend overlay entries should use the same canonical order"
    );
}
