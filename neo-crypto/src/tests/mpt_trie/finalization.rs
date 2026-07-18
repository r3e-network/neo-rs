use super::*;
use crate::mpt_trie::{MptError, MptStoreLookup};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

enum HistoryChange<'a> {
    Put(&'a [u8], &'a [u8]),
    Delete(&'a [u8]),
}

fn apply_history_changes<S: MptStoreSnapshot + 'static>(
    trie: &mut Trie<S>,
    changes: &[HistoryChange<'_>],
) {
    for change in changes {
        match change {
            HistoryChange::Put(key, value) => trie.put(key, value).unwrap(),
            HistoryChange::Delete(key) => assert!(trie.delete(key).unwrap()),
        }
    }
}

fn assert_values_at_root<S: MptStoreSnapshot + 'static>(
    store: Arc<S>,
    root: UInt256,
    expected: &[(&[u8], Option<&[u8]>)],
) {
    let mut trie = Trie::new(store, Some(root), true);
    for &(key, expected_value) in expected {
        assert_eq!(trie.get(key).unwrap().as_deref(), expected_value);
    }
}

struct FailOnceBatchStore {
    inner: MockStore,
    fail_next_batch: AtomicBool,
    batch_reads: AtomicUsize,
}

impl FailOnceBatchStore {
    fn new() -> Self {
        Self {
            inner: MockStore::new(),
            fail_next_batch: AtomicBool::new(true),
            batch_reads: AtomicUsize::new(0),
        }
    }
}

impl MptStoreSnapshot for FailOnceBatchStore {
    fn try_get(&self, key: &[u8]) -> MptResult<Option<Vec<u8>>> {
        self.inner.try_get(key)
    }

    fn try_get_nodes_with_source(&self, keys: &[Vec<u8>]) -> MptResult<Vec<MptStoreLookup<Node>>> {
        self.batch_reads.fetch_add(1, Ordering::Relaxed);
        if self.fail_next_batch.swap(false, Ordering::Relaxed) {
            return Err(MptError::storage(
                "injected full-state finalization batch failure",
            ));
        }
        keys.iter()
            .map(|key| self.inner.try_get_node_with_source(key))
            .collect()
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> MptResult<()> {
        self.inner.put(key, value)
    }

    fn delete(&self, key: Vec<u8>) -> MptResult<()> {
        self.inner.delete(key)
    }
}

fn commit_history<S: MptStoreSnapshot + 'static>(
    mut trie: Trie<S>,
    entries: &[(&[u8], &[u8])],
) -> (Trie<S>, UInt256) {
    for (key, value) in entries {
        trie.put_with_scratch(key, value, &mut Vec::new()).unwrap();
        trie.checkpoint().unwrap();
    }
    let root = trie.try_root_hash().unwrap();
    trie.commit().unwrap();
    (trie, root)
}

#[test]
fn deferred_finalization_matches_eager_root_and_namespace() {
    let entries: &[(&[u8], &[u8])] = &[
        (b"alpha", b"one"),
        (b"beta", b"two"),
        (b"gamma", b"three"),
        (b"alpha", b"updated"),
    ];
    let eager_store = Arc::new(MockStore::new());
    let deferred_store = Arc::new(MockStore::new());
    let (_, eager_root) =
        commit_history(Trie::new_eager(eager_store.clone(), None, false), entries);
    let (_, deferred_root) = commit_history(
        Trie::new_batch(deferred_store.clone(), None, false),
        entries,
    );
    assert_eq!(eager_root, deferred_root);

    let eager = eager_store.get_data();
    let deferred = deferred_store.get_data();
    assert!(!eager.is_empty());
    assert_eq!(eager, deferred);
}

#[test]
fn full_state_batch_finalization_matches_eager_root_and_namespace() {
    let entries: &[(&[u8], &[u8])] = &[
        (b"alpha", b"one"),
        (b"beta", b"two"),
        (b"gamma", b"three"),
        (b"alpha", b"updated"),
    ];
    let eager_store = Arc::new(MockStore::new());
    let deferred_store = Arc::new(MockStore::new());
    let (_, eager_root) = commit_history(Trie::new_eager(eager_store.clone(), None, true), entries);
    let (_, deferred_root) = commit_history(
        Trie::new_batch_deferred_full_state(deferred_store.clone(), None, true),
        entries,
    );

    assert_eq!(eager_root, deferred_root);
    let eager = eager_store.get_data();
    let deferred = deferred_store.get_data();
    assert!(!eager.is_empty());
    assert_eq!(eager, deferred);
}

#[test]
fn full_state_batch_preserves_every_checkpoint_root() {
    let first_block = [
        HistoryChange::Put(b"alpha", b"one"),
        HistoryChange::Put(b"beta", b"two"),
        HistoryChange::Put(b"gamma", b"three"),
    ];
    let second_block = [
        HistoryChange::Put(b"alpha", b"updated"),
        HistoryChange::Delete(b"beta"),
        HistoryChange::Put(b"delta", b"four"),
    ];
    let third_block = [
        HistoryChange::Put(b"beta", b"restored"),
        HistoryChange::Delete(b"gamma"),
        HistoryChange::Put(b"alpha", b"final"),
    ];
    let eager_store = Arc::new(MockStore::new());
    let batch_store = Arc::new(MockStore::new());
    let mut eager = Trie::new_eager(eager_store.clone(), None, true);
    let mut batch = Trie::new_batch_deferred_full_state(batch_store.clone(), None, true);
    let mut roots = Vec::with_capacity(3);

    for changes in [&first_block[..], &second_block[..], &third_block[..]] {
        apply_history_changes(&mut eager, changes);
        apply_history_changes(&mut batch, changes);
        let eager_root = eager.try_root_hash().unwrap();
        let batch_root = batch.try_root_hash().unwrap();
        assert_eq!(eager_root, batch_root);
        eager.checkpoint().unwrap();
        batch.checkpoint().unwrap();
        roots.push(eager_root);
    }

    eager.commit().unwrap();
    batch.commit().unwrap();

    assert_values_at_root(
        batch_store.clone(),
        roots[0],
        &[
            (b"alpha", Some(b"one")),
            (b"beta", Some(b"two")),
            (b"gamma", Some(b"three")),
            (b"delta", None),
        ],
    );
    assert_values_at_root(
        batch_store.clone(),
        roots[1],
        &[
            (b"alpha", Some(b"updated")),
            (b"beta", None),
            (b"gamma", Some(b"three")),
            (b"delta", Some(b"four")),
        ],
    );
    assert_values_at_root(
        batch_store.clone(),
        roots[2],
        &[
            (b"alpha", Some(b"final")),
            (b"beta", Some(b"restored")),
            (b"gamma", None),
            (b"delta", Some(b"four")),
        ],
    );

    let eager_data = eager_store.get_data();
    let batch_data = batch_store.get_data();
    assert!(!batch_data.is_empty());
    assert_eq!(batch_data, eager_data);
}

#[test]
fn full_state_batch_commit_retries_failed_finalization_without_partial_write() {
    let store = Arc::new(FailOnceBatchStore::new());
    let mut trie = Trie::new_batch_deferred_full_state(store.clone(), None, true);
    trie.put(b"alpha", b"one").unwrap();
    trie.put(b"beta", b"two").unwrap();
    let expected_root = trie.try_root_hash().unwrap();

    trie.checkpoint()
        .expect("checkpoint only queues full-state finalization");
    let error = trie
        .commit()
        .expect_err("first full-state finalization lookup must fail");
    assert!(error.to_string().contains("injected full-state"));
    assert!(store.inner.get_data().is_empty());
    assert_eq!(store.batch_reads.load(Ordering::Relaxed), 1);
    assert_eq!(trie.try_root_hash().unwrap(), expected_root);

    trie.commit()
        .expect("full-state finalization must remain retryable");
    assert!(!store.inner.get_data().is_empty());
    assert_eq!(store.batch_reads.load(Ordering::Relaxed), 2);
    assert_eq!(trie.try_root_hash().unwrap(), expected_root);

    assert_values_at_root(
        store,
        expected_root,
        &[(b"alpha", Some(b"one")), (b"beta", Some(b"two"))],
    );
}

#[test]
fn deferred_find_before_commit_matches_after_checkpoint() {
    let store = Arc::new(MockStore::new());
    let mut trie = Trie::new_batch(store, None, false);
    trie.put(b"prefix-a", b"a").unwrap();
    trie.put(b"prefix-b", b"b").unwrap();
    trie.put(b"other", b"c").unwrap();
    let before = trie.find(b"prefix", None).unwrap();
    trie.checkpoint().unwrap();
    trie.commit().unwrap();
    let after = trie.find(b"prefix", None).unwrap();
    assert_eq!(before, after);
}

#[test]
fn repeated_leaf_reference_crosses_signed_varint_boundary_without_root_drift() {
    let store = Arc::new(MockStore::new());
    let mut trie = Trie::new_batch(store, None, false);
    for index in 0..130u16 {
        let key = index.to_be_bytes();
        trie.put(&key, b"same").unwrap();
    }
    let root = trie.try_root_hash().unwrap();
    trie.checkpoint().unwrap();
    trie.commit().unwrap();
    assert_eq!(trie.try_root_hash().unwrap(), root);
}

#[test]
fn deferred_finalization_is_distinct_from_eager_finalization() {
    let entries: &[(&[u8], &[u8])] = &[(b"a", b"1"), (b"b", b"2"), (b"c", b"3")];
    let eager_store = Arc::new(MockStore::new());
    let deferred_store = Arc::new(MockStore::new());
    let (_, eager_root) = commit_history(Trie::new_eager(eager_store, None, false), entries);
    let (_, deferred_root) = commit_history(Trie::new_batch(deferred_store, None, false), entries);
    assert_eq!(eager_root, deferred_root);
}

#[test]
fn deferred_finalization_retry_keeps_parent_reachable() {
    let store = Arc::new(MockStore::new());
    let mut trie = Trie::new_batch(store, None, false);
    trie.put(b"parent/child", b"value").unwrap();
    trie.checkpoint().unwrap();
    trie.commit().unwrap();
    trie.checkpoint().unwrap();
    assert_eq!(trie.get(b"parent/child").unwrap(), Some(b"value".to_vec()));
}

#[test]
fn transient_duplicate_hash_does_not_decrement_durable_reference() {
    let store = Arc::new(MockStore::new());
    let mut trie = Trie::new_batch(store.clone(), None, false);
    trie.put(b"one", b"same").unwrap();
    trie.put(b"two", b"same").unwrap();
    trie.checkpoint().unwrap();
    trie.commit().unwrap();
    assert_eq!(trie.get(b"one").unwrap(), Some(b"same".to_vec()));
    assert_eq!(trie.get(b"two").unwrap(), Some(b"same".to_vec()));
}

#[test]
fn prefix_splits_and_delete_compression_preserve_exact_namespace() {
    let store = Arc::new(MockStore::new());
    let mut trie = Trie::new_batch(store, None, false);
    trie.put(b"prefix-a", b"a").unwrap();
    trie.put(b"prefix-b", b"b").unwrap();
    trie.put(b"prefix-c", b"c").unwrap();
    trie.delete(b"prefix-b").unwrap();
    trie.checkpoint().unwrap();
    trie.commit().unwrap();
    assert_eq!(trie.find(b"prefix", None).unwrap().len(), 2);
    assert_eq!(trie.get(b"prefix-a").unwrap(), Some(b"a".to_vec()));
    assert_eq!(trie.get(b"prefix-c").unwrap(), Some(b"c".to_vec()));
}

#[test]
fn deterministic_randomized_eager_and_deferred_histories_are_byte_identical() {
    let eager_store = Arc::new(MockStore::new());
    let deferred_store = Arc::new(MockStore::new());
    let mut eager = Trie::new_eager(eager_store.clone(), None, false);
    let mut deferred = Trie::new_batch(deferred_store.clone(), None, false);
    for index in 0..64u8 {
        let key = [index, index.wrapping_mul(17)];
        let value = [index.wrapping_mul(3)];
        eager.put(&key, &value).unwrap();
        deferred.put(&key, &value).unwrap();
        if index % 5 == 0 {
            eager.delete(&key).unwrap();
            deferred.delete(&key).unwrap();
        }
        eager.checkpoint().unwrap();
        deferred.checkpoint().unwrap();
    }
    eager.commit().unwrap();
    deferred.commit().unwrap();
    assert_eq!(
        eager.try_root_hash().unwrap(),
        deferred.try_root_hash().unwrap()
    );
    assert_eq!(eager_store.get_data(), deferred_store.get_data());
}
