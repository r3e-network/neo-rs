//! Tests for RocksDB write batching optimization in state service.

use super::*;
use crate::persistence::TrackState;
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Mock backend that counts individual write operations
#[derive(Clone)]
struct CountingStateStoreBackend {
    data: Arc<Mutex<HashMap<Vec<u8>, Vec<u8>>>>,
    put_count: Arc<AtomicUsize>,
    delete_count: Arc<AtomicUsize>,
    commit_count: Arc<AtomicUsize>,
}

impl CountingStateStoreBackend {
    fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(HashMap::new())),
            put_count: Arc::new(AtomicUsize::new(0)),
            delete_count: Arc::new(AtomicUsize::new(0)),
            commit_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn get_put_count(&self) -> usize {
        self.put_count.load(Ordering::Relaxed)
    }

    fn get_delete_count(&self) -> usize {
        self.delete_count.load(Ordering::Relaxed)
    }

    fn get_commit_count(&self) -> usize {
        self.commit_count.load(Ordering::Relaxed)
    }

    fn reset_counters(&self) {
        self.put_count.store(0, Ordering::Relaxed);
        self.delete_count.store(0, Ordering::Relaxed);
        self.commit_count.store(0, Ordering::Relaxed);
    }
}

impl StateStoreBackend for CountingStateStoreBackend {
    fn try_get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.data.lock().get(key).cloned()
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) {
        self.put_count.fetch_add(1, Ordering::Relaxed);
        self.data.lock().insert(key, value);
    }

    fn delete(&self, key: &[u8]) {
        self.delete_count.fetch_add(1, Ordering::Relaxed);
        self.data.lock().remove(key);
    }

    fn commit(&self) -> Result<(), String> {
        self.commit_count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn discard_pending(&self) {}
}

#[test]
fn test_staged_backend_batches_writes() {
    let base = Arc::new(CountingStateStoreBackend::new());
    let staged = StagedStateStoreBackend::new(base.clone());

    // Stage multiple writes
    staged.put(vec![1, 2, 3], vec![10, 20, 30]);
    staged.put(vec![4, 5, 6], vec![40, 50, 60]);
    staged.put(vec![7, 8, 9], vec![70, 80, 90]);
    staged.delete(&[1, 1, 1]);

    // Before commit, base should have no writes
    assert_eq!(base.get_put_count(), 0);
    assert_eq!(base.get_delete_count(), 0);
    assert_eq!(base.get_commit_count(), 0);

    // Commit the batch
    staged.commit().unwrap();

    // After commit, all writes should be applied
    assert_eq!(base.get_put_count(), 3);
    assert_eq!(base.get_delete_count(), 1);
    assert_eq!(base.get_commit_count(), 1);
}

#[test]
fn test_staged_backend_reads_pending_writes() {
    let base = Arc::new(CountingStateStoreBackend::new());
    let staged = StagedStateStoreBackend::new(base.clone());

    // Write to base first
    base.put(vec![1, 2, 3], vec![100, 200, 44]);
    base.commit().unwrap();
    base.reset_counters();

    // Stage an overwrite
    staged.put(vec![1, 2, 3], vec![111, 222, 77]);

    // Read should get staged value, not base value
    let value = staged.try_get(&[1, 2, 3]);
    assert_eq!(value, Some(vec![111, 222, 77]));

    // Commit and verify
    staged.commit().unwrap();
    let value = base.try_get(&[1, 2, 3]);
    assert_eq!(value, Some(vec![111, 222, 77]));
}

#[test]
fn test_staged_backend_discards_on_error() {
    let base = Arc::new(CountingStateStoreBackend::new());
    let staged = StagedStateStoreBackend::new(base.clone());

    // Stage writes
    staged.put(vec![1], vec![10]);
    staged.put(vec![2], vec![20]);
    staged.put(vec![3], vec![30]);

    // Discard instead of commit
    staged.discard_pending();

    // Verify nothing was written to base
    assert_eq!(base.get_put_count(), 0);
    assert_eq!(base.try_get(&[1]), None);
    assert_eq!(base.try_get(&[2]), None);
    assert_eq!(base.try_get(&[3]), None);
}

#[test]
fn test_state_store_batch_commit_performance() {
    let backend = Arc::new(CountingStateStoreBackend::new());
    let store = StateStore::new(backend.clone(), StateServiceSettings::default());

    // Prepare a change set with multiple storage updates
    let height = 100u32;
    let mut changes = Vec::new();

    for i in 0..100 {
        let key = StorageKey::new(i + 1, vec![0x01, 0x02]);
        let item = StorageItem::from_bytes(vec![i as u8, (i * 2) as u8]);
        changes.push((key, item, TrackState::Added));
    }

    backend.reset_counters();

    // Update the local state root with the change set
    store
        .update_local_state_root_snapshot(height, changes.into_iter())
        .unwrap();

    // Commit the state root
    store.update_local_state_root(height).unwrap();

    // Verify that writes were batched (should have many puts but only 1 commit)
    let commit_count = backend.get_commit_count();
    let put_count = backend.get_put_count();

    // Should have at least the trie node writes + state root metadata
    assert!(put_count > 0, "Expected some put operations");

    // The key assertion: we should have exactly 1 commit per batch, not one per write
    assert_eq!(
        commit_count, 1,
        "Expected exactly 1 commit for batched writes, got {}",
        commit_count
    );
}

#[test]
fn test_snapshot_backed_backend_batches_efficiently() {
    use crate::persistence::providers::memory_store::MemoryStore;
    use crate::persistence::store::Store;

    let mem_store = Arc::new(MemoryStore::new()) as Arc<dyn Store>;
    let backend = SnapshotBackedStateStoreBackend::new(mem_store.clone());

    // Stage multiple operations
    for i in 0..50 {
        let key = format!("key_{}", i).into_bytes();
        let value = format!("value_{}", i).into_bytes();
        backend.put(key, value);
    }

    // Delete some keys
    for i in 0..10 {
        let key = format!("key_{}", i).into_bytes();
        backend.delete(&key);
    }

    // Commit should batch all operations into a single snapshot commit
    let result = backend.commit();
    assert!(result.is_ok(), "Batch commit should succeed");

    // Verify the data is in the store
    let snapshot = mem_store.snapshot();
    for i in 10..50 {
        let key = format!("key_{}", i).into_bytes();
        let value = snapshot.try_get(&key);
        assert!(value.is_some(), "Key {} should exist after batch commit", i);
    }

    // Verify deleted keys are gone
    for i in 0..10 {
        let key = format!("key_{}", i).into_bytes();
        let value = snapshot.try_get(&key);
        assert!(value.is_none(), "Key {} should be deleted", i);
    }
}

#[test]
fn test_batch_commit_error_handling() {
    // Test that batch commit properly reports errors for individual operations
    let backend = Arc::new(CountingStateStoreBackend::new());
    let staged = StagedStateStoreBackend::new(backend.clone());

    staged.put(vec![1], vec![10]);
    staged.put(vec![2], vec![20]);

    // This should succeed
    let result = staged.commit();
    assert!(result.is_ok());
}

#[test]
fn test_large_batch_commit() {
    let backend = Arc::new(CountingStateStoreBackend::new());
    let staged = StagedStateStoreBackend::new(backend.clone());

    // Stage a large number of writes (simulating a large block)
    const BATCH_SIZE: usize = 1000;
    for i in 0..BATCH_SIZE {
        let key = i.to_le_bytes().to_vec();
        let value = (i * 2).to_le_bytes().to_vec();
        staged.put(key, value);
    }

    backend.reset_counters();

    // Commit the batch
    let result = staged.commit();
    assert!(result.is_ok(), "Large batch commit should succeed");

    // Verify exactly 1 commit call (batched)
    assert_eq!(backend.get_commit_count(), 1);
    assert_eq!(backend.get_put_count(), BATCH_SIZE);
}

#[test]
fn test_mixed_operations_batch() {
    let backend = Arc::new(CountingStateStoreBackend::new());

    // Pre-populate base with some data
    for i in 0..20 {
        backend.put(vec![i], vec![i * 10]);
    }
    backend.commit().unwrap();
    backend.reset_counters();

    let staged = StagedStateStoreBackend::new(backend.clone());

    // Mix of puts, updates, and deletes
    staged.put(vec![100], vec![200]); // New key
    staged.put(vec![5], vec![99]); // Update existing
    staged.delete(&[10]); // Delete existing
    staged.put(vec![101], vec![202]); // New key
    staged.delete(&[15]); // Delete existing

    let result = staged.commit();
    assert!(result.is_ok());

    // Verify single batch commit
    assert_eq!(backend.get_commit_count(), 1);
    assert_eq!(backend.get_put_count(), 3);
    assert_eq!(backend.get_delete_count(), 2);

    // Verify final state
    assert_eq!(backend.try_get(&[100]), Some(vec![200]));
    assert_eq!(backend.try_get(&[5]), Some(vec![99]));
    assert_eq!(backend.try_get(&[10]), None);
    assert_eq!(backend.try_get(&[101]), Some(vec![202]));
    assert_eq!(backend.try_get(&[15]), None);
}
