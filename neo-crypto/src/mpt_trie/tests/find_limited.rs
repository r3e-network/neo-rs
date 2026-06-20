use super::*;

// === Bounded find (find_visit / find_limited) ===

/// Store wrapper that counts `try_get` resolutions so the bounded-find
/// tests can prove the traversal stops touching the backing store once
/// the visitor requests a stop (instrumented early-stop, not timing).
struct CountingStore {
    data: Mutex<HashMap<Vec<u8>, Vec<u8>>>,
    gets: std::sync::atomic::AtomicUsize,
}

impl CountingStore {
    fn new() -> Self {
        Self {
            data: Mutex::new(HashMap::new()),
            gets: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    fn get_count(&self) -> usize {
        self.gets.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn reset_count(&self) {
        self.gets.store(0, std::sync::atomic::Ordering::Relaxed);
    }
}

impl MptStoreSnapshot for CountingStore {
    fn try_get(&self, key: &[u8]) -> MptResult<Option<Vec<u8>>> {
        self.gets.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(self.data.lock().get(key).cloned())
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> MptResult<()> {
        self.data.lock().insert(key, value);
        Ok(())
    }

    fn delete(&self, key: Vec<u8>) -> MptResult<()> {
        self.data.lock().remove(&key);
        Ok(())
    }
}

/// Seeds 64 single-byte keys (`0x00..=0x3F`), commits them, and reopens
/// the trie from the root hash alone so every traversal step must
/// resolve nodes through the counting store.
fn seeded_counting_trie(store: &Arc<CountingStore>) -> Trie<CountingStore> {
    let mut writer = Trie::new(Arc::clone(store), None, true);
    for byte in 0u8..64 {
        writer.put(&[byte], &[0xF0, byte]).unwrap();
    }
    let root = writer.root_hash().unwrap();
    writer.commit().unwrap();
    store.reset_count();
    Trie::new(Arc::clone(store), Some(root), true)
}

#[test]
fn find_visit_stops_exactly_at_visitor_bound() {
    let store = Arc::new(CountingStore::new());
    let mut trie = seeded_counting_trie(&store);

    let mut visited = Vec::new();
    trie.find_visit(&[], None, |entry| {
        visited.push(entry);
        visited.len() < 3
    })
    .unwrap();
    // The visitor was invoked exactly three times even though 64
    // entries live under the prefix.
    assert_eq!(visited.len(), 3);

    // And the bounded prefix matches the head of the full enumeration.
    let mut full_trie = seeded_counting_trie(&store);
    let all = full_trie.find(&[], None).unwrap();
    assert_eq!(all.len(), 64);
    assert_eq!(visited.as_slice(), &all[..3]);
}

#[test]
fn find_limited_matches_full_enumeration_prefix() {
    let store = Arc::new(CountingStore::new());
    let mut trie = seeded_counting_trie(&store);
    let all = trie.find(&[], None).unwrap();
    assert_eq!(all.len(), 64);

    for limit in [1usize, 2, 17, 64, 1000] {
        let mut bounded_trie = seeded_counting_trie(&store);
        let bounded = bounded_trie.find_limited(&[], None, limit).unwrap();
        let expected = &all[..limit.min(all.len())];
        assert_eq!(bounded.as_slice(), expected, "limit {limit}");
    }

    // Resuming from a key keeps the same bounded-prefix property.
    let resumed_all = trie.find(&[], Some(&[0x10])).unwrap();
    let mut resumed_trie = seeded_counting_trie(&store);
    let resumed_two = resumed_trie.find_limited(&[], Some(&[0x10]), 2).unwrap();
    assert_eq!(resumed_two.as_slice(), &resumed_all[..2]);
}

#[test]
fn find_limited_skips_resolving_unvisited_subtrees() {
    // Full sweep: every leaf node must be resolved from the store.
    let store_full = Arc::new(CountingStore::new());
    let mut full_trie = seeded_counting_trie(&store_full);
    assert_eq!(full_trie.find(&[], None).unwrap().len(), 64);
    let gets_full = store_full.get_count();

    // Bounded sweep: only the path to the first leaf is resolved.
    let store_one = Arc::new(CountingStore::new());
    let mut one_trie = seeded_counting_trie(&store_one);
    assert_eq!(one_trie.find_limited(&[], None, 1).unwrap().len(), 1);
    let gets_one = store_one.get_count();

    assert!(
        gets_one < gets_full / 2,
        "bounded traversal must resolve far fewer nodes \
         (limit-1 resolved {gets_one}, full sweep resolved {gets_full})"
    );
}

#[test]
fn find_limited_zero_visits_nothing() {
    let store = Arc::new(CountingStore::new());
    let mut trie = seeded_counting_trie(&store);
    let entries = trie.find_limited(&[], None, 0).unwrap();
    assert!(entries.is_empty());
    assert_eq!(
        store.get_count(),
        0,
        "a zero limit must not touch the store at all"
    );
}
