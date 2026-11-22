use super::*;
use crate::mpt_trie::Trie;
use crate::mpt_trie::tests::mpt_tests::MockStore;
use std::sync::Barrier;
use std::sync::atomic::AtomicUsize;
use std::thread;

const NAMESPACE: ProofCacheNamespace = ProofCacheNamespace::new(0x4e45_4f00);

/// Commits `entries` into a fresh full-state trie, retaining every root.
fn build_trie(entries: &[(&[u8], Vec<u8>)]) -> (Arc<MockStore>, Trie<MockStore>, CommittedRoot) {
    let store = Arc::new(MockStore::new());
    let mut trie = Trie::new_with_node_cache(Arc::clone(&store), None, true, None);
    for (key, value) in entries {
        trie.put(key, value).expect("stage value");
    }
    trie.commit().expect("commit trie");
    let root = trie.committed_root().expect("committed root");
    (store, trie, root)
}

/// Reopens a trie at a previously committed root of the same store.
fn reopen(store: &Arc<MockStore>, root: CommittedRoot) -> Trie<MockStore> {
    Trie::new_with_node_cache(Arc::clone(store), Some(root.hash()), true, None)
}

fn key(root: CommittedRoot, namespace: ProofCacheNamespace, raw: &[u8]) -> ProofCacheKey {
    ProofCacheKey::new(root, namespace, raw)
}

fn bounded(entries: usize, bytes: usize, entry_bytes: usize) -> ProofCacheConfig {
    ProofCacheConfig::new(
        NonZeroUsize::new(entries).expect("entry bound"),
        NonZeroUsize::new(bytes).expect("byte bound"),
        NonZeroUsize::new(entry_bytes).expect("entry byte bound"),
    )
}

#[test]
fn repeated_request_returns_the_same_bundle() {
    let (_store, mut trie, root) = build_trie(&[(b"account", b"value".to_vec())]);
    let cache = ProofCache::with_defaults();

    let first = trie
        .get_proof_cached(&cache, root, NAMESPACE, b"account")
        .expect("first proof")
        .expect("key present");
    let second = trie
        .get_proof_cached(&cache, root, NAMESPACE, b"account")
        .expect("second proof")
        .expect("key present");

    assert!(Arc::ptr_eq(&first, &second));
    let stats = cache.stats();
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.admissions, 1);
    assert_eq!(stats.constructions, 1);
    assert_eq!(stats.entries, 1);
    assert!(stats.bytes > first.byte_len(), "entry must carry overhead");
    assert!(stats.mean_construction_time().is_some());
    assert!(!first.is_empty());
}

#[test]
fn cached_proof_verifies_against_the_committed_root() {
    let (_store, mut trie, root) = build_trie(&[
        (b"account", b"value".to_vec()),
        (b"other", b"payload".to_vec()),
    ]);
    let cache = ProofCache::with_defaults();

    let bundle = trie
        .get_proof_cached(&cache, root, NAMESPACE, b"account")
        .expect("proof")
        .expect("key present");

    assert_eq!(
        Trie::<MockStore>::verify_proof(root.hash(), b"account", &bundle.to_hash_set())
            .expect("verify"),
        b"value".to_vec()
    );
    assert!(
        Trie::<MockStore>::verify_proof(root.hash(), b"absent", &bundle.to_hash_set()).is_err(),
        "a proof must not resolve an unrelated key"
    );
    // The cached bundle must describe the same nodes the uncached path does.
    let uncached = trie
        .try_get_proof(b"account")
        .expect("uncached proof")
        .expect("key present");
    assert_eq!(uncached, bundle.to_hash_set());
}

#[test]
fn distinct_roots_keep_distinct_entries() {
    let store = Arc::new(MockStore::new());
    let mut trie = Trie::new_with_node_cache(Arc::clone(&store), None, true, None);
    trie.put(b"account", b"v1").expect("stage first block");
    trie.commit().expect("commit first block");
    let first_root = trie.committed_root().expect("first root");

    trie.put(b"account", b"v2").expect("stage second block");
    trie.commit().expect("commit second block");
    let second_root = trie.committed_root().expect("second root");
    assert_ne!(first_root.hash(), second_root.hash());

    let cache = ProofCache::with_defaults();
    let mut old = reopen(&store, first_root);
    let mut current = reopen(&store, second_root);

    let old_proof = old
        .get_proof_cached(&cache, first_root, NAMESPACE, b"account")
        .expect("first root proof")
        .expect("key present");
    let current_proof = current
        .get_proof_cached(&cache, second_root, NAMESPACE, b"account")
        .expect("second root proof")
        .expect("key present");
    assert_ne!(old_proof.nodes(), current_proof.nodes());
    assert_eq!(cache.stats().entries, 2);

    // The entry keyed by the old root is still served after the current
    // root advanced: advancing the head must not invalidate history.
    let replay = old
        .get_proof_cached(&cache, first_root, NAMESPACE, b"account")
        .expect("replay")
        .expect("key present");
    assert!(Arc::ptr_eq(&old_proof, &replay));
    assert_eq!(cache.stats().hits, 1);
}

#[test]
fn distinct_keys_keep_distinct_entries() {
    let (_store, mut trie, root) =
        build_trie(&[(b"alpha", b"one".to_vec()), (b"beta", b"two".to_vec())]);
    let cache = ProofCache::with_defaults();

    let alpha = trie
        .get_proof_cached(&cache, root, NAMESPACE, b"alpha")
        .expect("alpha proof")
        .expect("key present");
    let beta = trie
        .get_proof_cached(&cache, root, NAMESPACE, b"beta")
        .expect("beta proof")
        .expect("key present");

    assert_ne!(alpha.nodes(), beta.nodes());
    let stats = cache.stats();
    assert_eq!(stats.entries, 2);
    assert_eq!(stats.admissions, 2);
    assert_eq!(stats.hits, 0);
}

#[test]
fn namespace_separates_identical_roots() {
    let (_store, mut trie, root) = build_trie(&[(b"account", b"value".to_vec())]);
    let cache = ProofCache::with_defaults();
    let other_namespace = ProofCacheNamespace::new(NAMESPACE.id() + 1);

    let first = trie
        .get_proof_cached(&cache, root, NAMESPACE, b"account")
        .expect("first namespace")
        .expect("key present");
    let second = trie
        .get_proof_cached(&cache, root, other_namespace, b"account")
        .expect("second namespace")
        .expect("key present");

    assert!(!Arc::ptr_eq(&first, &second));
    let stats = cache.stats();
    assert_eq!(stats.entries, 2);
    assert_eq!(stats.hits, 0);
    assert_ne!(
        key(root, NAMESPACE, b"account"),
        key(root, other_namespace, b"account")
    );
}

#[test]
fn entry_bound_evicts_the_least_recently_used_entry() {
    let (_store, mut trie, root) = build_trie(&[
        (b"one", b"1".to_vec()),
        (b"two", b"2".to_vec()),
        (b"three", b"3".to_vec()),
    ]);
    let cache = ProofCache::new(bounded(2, 8 * 1024 * 1024, 1024 * 1024));

    for raw in [b"one".as_slice(), b"two", b"three"] {
        trie.get_proof_cached(&cache, root, NAMESPACE, raw)
            .expect("proof")
            .expect("key present");
    }

    let stats = cache.stats();
    assert_eq!(stats.entries, 2);
    assert_eq!(stats.evictions, 1);
    assert!(!cache.contains(&key(root, NAMESPACE, b"one")));
    assert!(cache.contains(&key(root, NAMESPACE, b"three")));

    // The evicted key is rebuilt rather than served stale.
    let rebuilt = trie
        .get_proof_cached(&cache, root, NAMESPACE, b"one")
        .expect("rebuilt proof")
        .expect("key present");
    assert!(!rebuilt.is_empty());
    assert_eq!(
        Trie::<MockStore>::verify_proof(root.hash(), b"one", &rebuilt.to_hash_set())
            .expect("verify rebuilt proof"),
        b"1".to_vec()
    );
    assert_eq!(cache.stats().entries, 2);
}

#[test]
fn byte_bound_evicts_older_entries() {
    let (_store, mut trie, root) = build_trie(&[(b"one", b"1".to_vec()), (b"two", b"2".to_vec())]);
    let probe = ProofCache::with_defaults();
    trie.get_proof_cached(&probe, root, NAMESPACE, b"one")
        .expect("probe proof")
        .expect("key present");
    let one_entry_bytes = probe.stats().bytes;
    assert!(one_entry_bytes > 0);

    let cache = ProofCache::new(bounded(16, one_entry_bytes, one_entry_bytes));
    trie.get_proof_cached(&cache, root, NAMESPACE, b"one")
        .expect("first proof")
        .expect("key present");
    trie.get_proof_cached(&cache, root, NAMESPACE, b"two")
        .expect("second proof")
        .expect("key present");

    let stats = cache.stats();
    assert!(stats.evictions >= 1);
    assert!(stats.bytes <= one_entry_bytes);
    assert!(stats.entries <= 1);
    assert!(!cache.contains(&key(root, NAMESPACE, b"one")));
}

#[test]
fn oversized_proof_is_returned_but_never_admitted() {
    let (_store, mut trie, root) =
        build_trie(&[(b"small", b"v".to_vec()), (b"large", vec![7u8; 4096])]);
    let cache = ProofCache::new(bounded(8, 1024 * 1024, 2048));

    let small = trie
        .get_proof_cached(&cache, root, NAMESPACE, b"small")
        .expect("small proof")
        .expect("key present");
    let admitted = cache.stats();
    assert_eq!(admitted.entries, 1);
    assert_eq!(admitted.oversized_rejects, 0);
    let working_set_bytes = admitted.bytes;

    let large = trie
        .get_proof_cached(&cache, root, NAMESPACE, b"large")
        .expect("large proof")
        .expect("key present");
    assert!(large.byte_len() > 2048, "fixture must exceed the bound");

    // The oversized proof is still usable by the caller ...
    assert_eq!(
        Trie::<MockStore>::verify_proof(root.hash(), b"large", &large.to_hash_set())
            .expect("verify"),
        vec![7u8; 4096]
    );
    // ... but it neither occupied the cache nor evicted the working set.
    let stats = cache.stats();
    assert_eq!(stats.oversized_rejects, 1);
    assert_eq!(stats.evictions, 0);
    assert_eq!(stats.entries, 1);
    assert_eq!(stats.bytes, working_set_bytes);
    assert!(!cache.contains(&key(root, NAMESPACE, b"large")));

    let hit = trie
        .get_proof_cached(&cache, root, NAMESPACE, b"small")
        .expect("cache hit")
        .expect("key present");
    assert!(Arc::ptr_eq(&hit, &small));
}

#[test]
fn concurrent_identical_requests_coalesce_into_one_traversal() {
    const THREADS: usize = 8;
    let cache = Arc::new(ProofCache::with_defaults());
    let root = CommittedRoot::from_committed(UInt256::from([0x11u8; 32]));
    let cache_key = key(root, NAMESPACE, b"shared");
    let builds = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(THREADS));

    let handles: Vec<_> = (0..THREADS)
        .map(|_| {
            let cache = Arc::clone(&cache);
            let cache_key = cache_key.clone();
            let builds = Arc::clone(&builds);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                let waiter_cache = Arc::clone(&cache);
                cache
                    .get_or_construct(&cache_key, || {
                        builds.fetch_add(1, Ordering::SeqCst);
                        // Hold the flight open until every other request has
                        // joined it, which makes coalescing observable
                        // instead of timing dependent.
                        let deadline = Instant::now() + Duration::from_secs(10);
                        while waiter_cache.stats().coalesced < (THREADS as u64 - 1)
                            && Instant::now() < deadline
                        {
                            thread::yield_now();
                        }
                        Ok(Some(vec![vec![1, 2, 3], vec![4, 5, 6]]))
                    })
                    .expect("build")
                    .expect("proof")
            })
        })
        .collect();

    let bundles: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().expect("thread"))
        .collect();

    assert_eq!(builds.load(Ordering::SeqCst), 1, "one traversal only");
    let stats = cache.stats();
    assert_eq!(stats.constructions, 1);
    assert_eq!(stats.coalesced, THREADS as u64 - 1);
    assert_eq!(stats.admissions, 1);
    assert_eq!(stats.entries, 1);
    assert!(
        bundles
            .iter()
            .all(|bundle| Arc::ptr_eq(bundle, &bundles[0])),
        "every caller must receive the leader's bundle"
    );
    assert_eq!(bundles[0].len(), 2);
}

#[test]
fn missing_key_is_never_cached() {
    let (_store, mut trie, root) = build_trie(&[(b"account", b"value".to_vec())]);
    let cache = ProofCache::with_defaults();

    assert!(
        trie.get_proof_cached(&cache, root, NAMESPACE, b"absent")
            .expect("first miss")
            .is_none()
    );
    assert!(
        trie.get_proof_cached(&cache, root, NAMESPACE, b"absent")
            .expect("second miss")
            .is_none()
    );

    let stats = cache.stats();
    assert_eq!(stats.entries, 0);
    assert_eq!(stats.bytes, 0);
    assert_eq!(stats.hits, 0);
    assert_eq!(stats.misses, 2);
    assert_eq!(stats.admissions, 0);
    assert_eq!(stats.constructions, 2);
}

#[test]
fn speculative_roots_are_rejected() {
    let store = Arc::new(MockStore::new());
    let mut trie = Trie::new_with_node_cache(Arc::clone(&store), None, true, None);
    trie.put(b"account", b"value").expect("stage value");

    // Uncommitted writes leave the root mutable.
    assert!(trie.committed_root().is_none());
    let cache = ProofCache::with_defaults();
    let asserted = CommittedRoot::from_committed(UInt256::from([0x22u8; 32]));
    let error = trie
        .get_proof_cached(&cache, asserted, NAMESPACE, b"account")
        .expect_err("mutable root must be rejected");
    assert!(error.to_string().contains("proof cache rejection"));
    assert_eq!(cache.stats().entries, 0);
    assert_eq!(cache.stats().constructions, 0);

    trie.commit().expect("commit trie");
    let root = trie.committed_root().expect("root is now committed");
    assert_ne!(root.hash(), asserted.hash());

    // A marker that does not match the trie's own root is rejected too.
    let error = trie
        .get_proof_cached(&cache, asserted, NAMESPACE, b"account")
        .expect_err("mismatched root must be rejected");
    assert!(error.to_string().contains("does not match trie root"));
    assert_eq!(cache.stats().entries, 0);

    // The same request with the real committed root is served.
    trie.get_proof_cached(&cache, root, NAMESPACE, b"account")
        .expect("committed proof")
        .expect("key present");
    assert_eq!(cache.stats().entries, 1);
}

#[test]
fn empty_trie_has_no_committed_root() {
    let store = Arc::new(MockStore::new());
    let trie = Trie::new_with_node_cache(store, None, true, None);
    assert!(trie.committed_root().is_none());
}

#[test]
fn clear_drops_entries_and_bytes() {
    let (_store, mut trie, root) = build_trie(&[(b"account", b"value".to_vec())]);
    let cache = ProofCache::with_defaults();
    trie.get_proof_cached(&cache, root, NAMESPACE, b"account")
        .expect("proof")
        .expect("key present");
    assert_eq!(cache.len(), 1);
    assert!(cache.byte_len() > 0);

    cache.clear();
    assert!(cache.is_empty());
    assert_eq!(cache.byte_len(), 0);
    assert_eq!(cache.stats().entries, 0);
    // Counters are cumulative and survive a clear.
    assert_eq!(cache.stats().admissions, 1);
}

#[test]
fn config_clamps_entry_bound_to_byte_bound() {
    let config = bounded(4, 1024, 4096);
    assert_eq!(config.max_entry_bytes().get(), 1024);
    assert_eq!(config.max_entries().get(), 4);
}

#[test]
fn key_carries_root_namespace_version_and_storage_key() {
    let root = CommittedRoot::from_committed(UInt256::from([3u8; 32]));
    let cache_key = key(root, NAMESPACE, b"storage-key");
    assert_eq!(cache_key.root(), root.hash());
    assert_eq!(cache_key.namespace(), NAMESPACE);
    assert_eq!(cache_key.format_version(), PROOF_FORMAT_VERSION);
    assert_eq!(cache_key.storage_key(), b"storage-key");
    assert_ne!(cache_key, key(root, NAMESPACE, b"other-key"));
}

#[test]
fn failed_build_is_not_served_to_waiters() {
    let cache = ProofCache::with_defaults();
    let cache_key = key(
        CommittedRoot::from_committed(UInt256::from([4u8; 32])),
        NAMESPACE,
        b"retry",
    );
    let error = cache
        .get_or_construct(&cache_key, || Err(MptError::storage("traversal failed")))
        .expect_err("leader failure");
    assert!(error.to_string().contains("traversal failed"));
    assert_eq!(cache.stats().entries, 0);

    // A retry after the failure is served by a new traversal.
    let bundle = cache
        .get_or_construct(&cache_key, || Ok(Some(vec![vec![9]])))
        .expect("retry")
        .expect("proof");
    assert_eq!(bundle.len(), 1);
    assert_eq!(cache.stats().constructions, 2);
}

#[test]
fn panicking_leader_releases_its_flight() {
    let cache = ProofCache::with_defaults();
    let cache_key = key(
        CommittedRoot::from_committed(UInt256::from([5u8; 32])),
        NAMESPACE,
        b"panic",
    );
    // The caught unwind prints one panic message; the flight must still be
    // released so a later request can build instead of blocking forever.
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        cache.get_or_construct(&cache_key, || panic!("traversal panic"))
    }));
    assert!(panicked.is_err());

    let bundle = cache
        .get_or_construct(&cache_key, || Ok(Some(vec![vec![7]])))
        .expect("retry after panic")
        .expect("proof");
    assert_eq!(bundle.len(), 1);
    assert_eq!(cache.stats().entries, 1);
}
