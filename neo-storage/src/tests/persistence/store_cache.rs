use super::*;
use crate::StorageError;
use crate::persistence::providers::memory_store::MemoryStore;
use crate::persistence::read_only_store::{RawReadOnlyStore, ReadOnlyStore, ReadOnlyStoreGeneric};
use crate::persistence::seek_direction::SeekDirection;
use crate::persistence::store::{OnNewSnapshotDelegate, Store};
use crate::persistence::store_snapshot::StoreSnapshot;
use crate::persistence::write_store::WriteStore;
use crate::types::{StorageItem, StorageKey};
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Clone, Copy)]
enum OverlayMode {
    Unsupported,
    Fails,
    Borrowed,
    Materialized,
}

#[derive(Debug)]
struct OverlayContractStore {
    inner: MemoryStore,
    mode: OverlayMode,
    borrowed_overlay: Mutex<Vec<(Vec<u8>, Option<Vec<u8>>)>>,
    cloned_overlay: Mutex<Vec<(Vec<u8>, Option<Vec<u8>>)>>,
    cloned_raw_overlay_attempts: AtomicUsize,
}

impl OverlayContractStore {
    fn new(mode: OverlayMode) -> Self {
        Self {
            inner: MemoryStore::new(),
            mode,
            borrowed_overlay: Mutex::new(Vec::new()),
            cloned_overlay: Mutex::new(Vec::new()),
            cloned_raw_overlay_attempts: AtomicUsize::new(0),
        }
    }

    fn borrowed_overlay(&self) -> Vec<(Vec<u8>, Option<Vec<u8>>)> {
        self.borrowed_overlay.lock().clone()
    }

    fn cloned_overlay(&self) -> Vec<(Vec<u8>, Option<Vec<u8>>)> {
        self.cloned_overlay.lock().clone()
    }

    fn cloned_raw_overlay_attempts(&self) -> usize {
        self.cloned_raw_overlay_attempts.load(Ordering::Relaxed)
    }
}

impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for OverlayContractStore {
    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        self.inner.try_get(key)
    }

    fn find(
        &self,
        key_prefix: Option<&Vec<u8>>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        self.inner.find(key_prefix, direction)
    }
}

impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for OverlayContractStore {
    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        self.inner.try_get(key)
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
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
    fn snapshot(&self) -> Arc<dyn StoreSnapshot> {
        self.inner.snapshot()
    }

    fn on_new_snapshot(&self, handler: OnNewSnapshotDelegate) {
        self.inner.on_new_snapshot(handler);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_raw_overlay_store(&self) -> Option<&dyn crate::persistence::RawOverlayStore> {
        if matches!(self.mode, OverlayMode::Unsupported) {
            None
        } else {
            Some(self)
        }
    }
}

impl crate::persistence::RawOverlayStore for OverlayContractStore {
    fn try_commit_raw_overlay(
        &self,
        overlay: &[(Vec<u8>, Option<Vec<u8>>)],
    ) -> crate::StorageResult<bool> {
        self.cloned_raw_overlay_attempts
            .fetch_add(1, Ordering::Relaxed);
        match self.mode {
            OverlayMode::Unsupported => Ok(false),
            OverlayMode::Fails => Err(StorageError::backend("raw overlay failed")),
            OverlayMode::Borrowed => {
                panic!("borrowed overlay mode must not materialize the cloned raw overlay")
            }
            OverlayMode::Materialized => {
                self.cloned_overlay.lock().extend_from_slice(overlay);
                Ok(true)
            }
        }
    }

    fn try_commit_borrowed_raw_overlay(
        &self,
        visit: &mut dyn FnMut(&mut dyn FnMut(&[u8], Option<&[u8]>)),
    ) -> crate::StorageResult<bool> {
        if !matches!(self.mode, OverlayMode::Borrowed) {
            return Ok(false);
        }

        let mut overlay = self.borrowed_overlay.lock();
        let mut collect = |key: &[u8], value: Option<&[u8]>| {
            overlay.push((key.to_vec(), value.map(<[u8]>::to_vec)));
        };
        visit(&mut collect);
        Ok(true)
    }
}

#[test]
fn unsupported_raw_overlay_falls_back_to_snapshot_commit() {
    let store: Arc<dyn Store> = Arc::new(OverlayContractStore::new(OverlayMode::Unsupported));
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
    let concrete = Arc::new(OverlayContractStore::new(OverlayMode::Unsupported));
    let store: Arc<dyn Store> = concrete.clone();
    let key = StorageKey::new(7, vec![0x05]);

    let mut writer = StoreCache::new_from_store(Arc::clone(&store), false);
    writer.add(key.clone(), StorageItem::from_bytes(vec![0xEE]));

    writer
        .try_commit()
        .expect("snapshot fallback should persist changes");

    assert_eq!(
        concrete.cloned_raw_overlay_attempts(),
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
    let store: Arc<dyn Store> = Arc::new(OverlayContractStore::new(OverlayMode::Fails));
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
fn borrowed_raw_overlay_fast_path_commits_without_cloned_overlay() {
    let concrete = Arc::new(OverlayContractStore::new(OverlayMode::Borrowed));
    let store: Arc<dyn Store> = concrete.clone();
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
    let mut borrowed_overlay = concrete.borrowed_overlay();
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
    let concrete = Arc::new(OverlayContractStore::new(OverlayMode::Borrowed));
    let store: Arc<dyn Store> = concrete.clone();
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

    let borrowed_overlay = concrete.borrowed_overlay();
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
    let concrete = Arc::new(OverlayContractStore::new(OverlayMode::Materialized));
    let store: Arc<dyn Store> = concrete.clone();
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
        concrete.cloned_overlay(),
        vec![
            (low_key.to_array(), Some(vec![0x01])),
            (mid_key.to_array(), Some(vec![0x02])),
            (high_key.to_array(), Some(vec![0x03])),
        ],
        "materialized backend overlay entries should use the same canonical order"
    );
}
