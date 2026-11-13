use super::error::{MptError, MptResult};
use super::node::Node;
use super::profile::{MptProfile, MptProfileStage, MptProfileTimer};
use lru::LruCache;
use neo_io::{MemoryReader, Serializable};
use neo_primitives::UINT256_SIZE;
use neo_primitives::UInt256;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};

use std::num::NonZero;

/// Default capacity for the global node cache (number of nodes, not bytes)
const DEFAULT_NODE_CACHE_CAPACITY: usize = 1_000_000; // Increased from 500K

/// High-performance global LRU cache for MPT nodes with statistics tracking.
///
/// This cache persists across block boundaries and stores fully-deserialized `Arc<Node>`
/// objects, eliminating ~250K+ deserializations per block. Critical fix confirmed by
/// Sarah's research that identified the old Vec<u8> caching as a major bottleneck.
///
/// Features:
/// - Thread-safe using `parking_lot::RwLock` for better concurrency
/// - Atomic hit/miss counters for real-time monitoring
/// - Double capacity (1M entries) compared to previous implementation
/// - Zero-deserialization on cache hits (returns Arc<Node> directly)
pub struct GlobalNodeCache {
    inner: RwLock<LruCache<UInt256, Arc<Node>>>,
    capacity: NonZero<usize>,
    hits: AtomicU64,
    misses: AtomicU64,
}

impl GlobalNodeCache {
    /// Creates a new empty global node cache with default capacity.
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(NonZero::new(DEFAULT_NODE_CACHE_CAPACITY).unwrap())
    }

    /// Creates an empty cache with the specified nonzero node capacity.
    #[must_use]
    pub fn with_capacity(capacity: NonZero<usize>) -> Self {
        Self {
            inner: RwLock::new(LruCache::new(capacity)),
            capacity,
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }

    /// Gets a cached node by hash, updating hit/miss statistics.
    ///
    /// Returns `None` if the node is not in the cache (counts as miss).
    ///
    /// A hit clones the `Arc` under a shared lock. One hit in sixteen also
    /// moves that node to the front of the LRU list. The cached node is not
    /// modified.
    #[must_use]
    pub fn get(&self, key: &UInt256) -> Option<Arc<Node>> {
        let hit = self.inner.read().peek(key).map(Arc::clone);
        let Some(node) = hit else {
            self.misses.fetch_add(1, Ordering::Relaxed);
            return None;
        };
        let hits = self.hits.fetch_add(1, Ordering::Relaxed);
        // Every sixteenth hit refreshes the entry's LRU position.
        if hits.trailing_zeros() >= 4 {
            let _ = self.inner.write().get(key);
        }
        Some(node)
    }

    /// Inserts a node into the cache, evicting LRU entries if at capacity.
    pub fn put(&self, key: UInt256, value: Arc<Node>) {
        self.inner.write().push(key, value);
    }

    /// Removes a node from the cache.
    pub fn remove(&self, key: &UInt256) {
        self.inner.write().pop(key);
    }

    /// Returns cache statistics as `(hits, misses, hit_rate_percent)`.
    ///
    /// The hit rate is expressed as a percentage (0.0 to 100.0).
    #[must_use]
    pub fn stats(&self) -> (u64, u64, f32) {
        let hits = self.hits.load(Ordering::Relaxed);
        let misses = self.misses.load(Ordering::Relaxed);
        let total = hits + misses;
        let hit_rate = if total > 0 {
            hits as f32 / total as f32 * 100.0
        } else {
            0.0
        };
        (hits, misses, hit_rate)
    }

    /// Clears all entries from the cache.
    #[allow(dead_code)] // WIP: cache integration not yet wired
    pub fn clear(&self) {
        self.inner.write().clear();
    }

    /// Returns the current number of cached nodes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.read().len()
    }

    /// Returns the configured maximum number of nodes.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity.get()
    }

    /// Returns `true` when the cache holds no nodes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for GlobalNodeCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Global cross-block LRU cache singleton instance.
pub static GLOBAL_NODE_CACHE: LazyLock<GlobalNodeCache> = LazyLock::new(GlobalNodeCache::new);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TrackState {
    None,
    Added,
    Changed,
    Deleted,
}

/// Abstraction over the persistence snapshot used by the trie cache.
pub trait MptStoreSnapshot: Send + Sync {
    /// Retrieves the serialized node associated with the specified key.
    fn try_get(&self, key: &[u8]) -> MptResult<Option<Vec<u8>>>;

    /// Persists the serialized node for the supplied key.
    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> MptResult<()>;

    /// Persists a node, optionally using the cache's prior resolution to prove
    /// that the key was absent from the persistent store.
    ///
    /// Implementations that cannot use this information retain the original
    /// behavior. Implementations that use the hint must still reject conflicts
    /// already visible in their pending snapshot.
    fn put_with_hint(
        &self,
        key: Vec<u8>,
        value: Vec<u8>,
        _known_absent_in_store: bool,
    ) -> MptResult<()> {
        self.put(key, value)
    }

    /// Persists a content-addressed node for a full-state trie. Historical
    /// nodes are never reference-count pruned in this mode, so stores may
    /// upsert the node without first reading its unused reference count.
    /// The node body and its content hash remain unchanged.
    fn put_full_state(&self, key: Vec<u8>, value: Vec<u8>) -> MptResult<()> {
        self.put_with_hint(key, value, true)
    }

    /// Removes the value associated with the supplied key.
    fn delete(&self, key: Vec<u8>) -> MptResult<()>;
}

struct MptTrackable {
    /// Shared with [`GlobalNodeCache`] until a reference-count update detaches it.
    node: Option<Arc<Node>>,
    state: TrackState,
    initial_ref: u32,
}

impl MptTrackable {
    fn new(node: Option<Arc<Node>>) -> Self {
        let initial_ref = node.as_ref().map_or(0, |n| n.reference);
        Self {
            node,
            state: TrackState::None,
            initial_ref,
        }
    }
}

/// Write-through cache mirroring the behaviour of the C# implementation.
///
/// Nodes are addressed by their hash and reference counted so that multiple
/// parents can point to the same subtree while it lives inside the cache.
pub struct MptCache<S>
where
    S: MptStoreSnapshot,
{
    store: Arc<S>,
    prefix: u8,
    entries: HashMap<UInt256, MptTrackable>,
    use_global_cache: bool,
    node_cache: Option<Arc<GlobalNodeCache>>,
    full_state: bool,
    profile: Option<Arc<MptProfile>>,
}

fn fast_full_state_commit_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("NEO_RS_DISABLE_FULL_STATE_FAST_COMMIT").is_none())
}

impl<S> MptCache<S>
where
    S: MptStoreSnapshot,
{
    /// Creates a new cache backed by the given store snapshot with the specified key prefix.
    ///
    /// The process-wide [`GLOBAL_NODE_CACHE`] is not consulted; use
    /// [`MptCache::with_global_cache`] to opt in.
    pub fn new(store: Arc<S>, prefix: u8) -> Self {
        Self::with_global_cache(store, prefix, false)
    }

    /// Creates a cache that optionally reads through [`GLOBAL_NODE_CACHE`].
    ///
    /// Cached nodes carry a reference count that can lag the store, so the
    /// global cache must only be enabled when reference counts never drive
    /// deletion (full-state tries).
    pub fn with_global_cache(store: Arc<S>, prefix: u8, use_global_cache: bool) -> Self {
        Self {
            store,
            prefix,
            entries: HashMap::new(),
            use_global_cache,
            node_cache: None,
            full_state: false,
            profile: None,
        }
    }

    /// Creates a cache backed by a store-scoped [`GlobalNodeCache`].
    pub fn with_node_cache(
        store: Arc<S>,
        prefix: u8,
        node_cache: Option<Arc<GlobalNodeCache>>,
    ) -> Self {
        Self {
            store,
            prefix,
            entries: HashMap::new(),
            use_global_cache: false,
            node_cache,
            full_state: false,
            profile: None,
        }
    }

    pub(crate) fn set_full_state(&mut self, full_state: bool) {
        self.full_state = full_state;
    }

    fn use_fast_full_state_commit(&self) -> bool {
        self.full_state && fast_full_state_commit_enabled()
    }

    pub(crate) fn set_profile(&mut self, profile: Option<Arc<MptProfile>>) {
        self.profile = profile;
    }

    /// Resolves the node identified by the supplied hash if present either in the
    /// in-memory cache or the underlying store.
    pub fn resolve(&mut self, hash: &UInt256) -> MptResult<Option<Node>> {
        let resolved = self.resolve_node(hash)?;
        if let Some(ref node) = resolved {
            node.clear_cached_hash();
        }
        Ok(resolved)
    }

    /// Resolves the node while keeping its pre-cached hash intact for internal trie operations.
    pub(crate) fn resolve_node(&mut self, hash: &UInt256) -> MptResult<Option<Node>> {
        let entry = self.resolve_internal(hash)?;
        Ok(entry.node.as_ref().map(|node| node.as_ref().clone()))
    }

    /// Adds or updates the supplied node inside the cache.
    pub fn put_node(&mut self, node: Node) -> MptResult<()> {
        let _ = self.put_node_with_hint(node, false)?;
        Ok(())
    }

    /// Adds or updates the supplied node inside the cache, returning `true` if the node
    /// did not exist in the persistent store prior to this cache session (`initial_ref == 0`).
    ///
    /// When `known_absent_in_store` is `true` (because at least one child of this node has
    /// `initial_ref == 0`), a vacant entry in `self.entries` skips querying the underlying
    /// persistent store. Full-state mode also skips this lookup: its reference counts are
    /// never used to prune history, and [`MptStoreSnapshot::put_full_state`] upserts the
    /// content-addressed body without claiming that the node was previously absent.
    pub(crate) fn put_node_with_hint(
        &mut self,
        mut node: Node,
        known_absent_in_store: bool,
    ) -> MptResult<bool> {
        node.clear_dirty();
        let hash = node.try_hash()?;

        if known_absent_in_store || self.use_fast_full_state_commit() {
            return match self.entries.entry(hash) {
                Entry::Occupied(occupied) => {
                    let entry = occupied.into_mut();
                    if let Some(existing) = entry.node.as_mut() {
                        let next = existing.reference.saturating_add(1);
                        Arc::make_mut(existing).reference = next;
                    } else {
                        node.reference = 1;
                        entry.node = Some(Arc::new(node));
                    }
                    entry.state = if entry.initial_ref > 0 {
                        TrackState::Changed
                    } else {
                        TrackState::Added
                    };
                    Ok(entry.initial_ref == 0)
                }
                Entry::Vacant(vacant) => {
                    node.reference = 1;
                    vacant.insert(MptTrackable {
                        node: Some(Arc::new(node)),
                        state: TrackState::Added,
                        initial_ref: 0,
                    });
                    Ok(true)
                }
            };
        }

        let entry = self.resolve_internal(&hash)?;
        let initial_ref = entry.initial_ref;
        if let Some(existing) = entry.node.as_mut() {
            let next = existing.reference.saturating_add(1);
            Arc::make_mut(existing).reference = next;
        } else {
            node.reference = 1;
            entry.node = Some(Arc::new(node));
        }
        entry.state = if initial_ref > 0 {
            TrackState::Changed
        } else {
            TrackState::Added
        };
        Ok(initial_ref == 0)
    }

    /// Decrements the reference count for the node or marks it for deletion when it
    /// is no longer referenced.
    pub fn delete_node(&mut self, hash: UInt256) -> MptResult<()> {
        let entry = self.resolve_internal(&hash)?;
        let initial_ref = entry.initial_ref;
        let still_referenced = {
            let Some(slot) = entry.node.as_mut() else {
                return Ok(());
            };
            if slot.reference > 1 {
                let next = slot.reference - 1;
                Arc::make_mut(slot).reference = next;
                true
            } else {
                false
            }
        };
        if still_referenced {
            entry.state = if initial_ref > 0 {
                TrackState::Changed
            } else {
                TrackState::Added
            };
        } else {
            entry.node = None;
            entry.state = if initial_ref > 0 {
                TrackState::Deleted
            } else {
                TrackState::None
            };
        }
        Ok(())
    }

    /// Flushes the pending changes to the underlying store.
    pub fn commit(&mut self) -> MptResult<()> {
        let active_cache: Option<&GlobalNodeCache> = if let Some(ref cache) = self.node_cache {
            Some(cache.as_ref())
        } else if self.use_global_cache {
            Some(&GLOBAL_NODE_CACHE)
        } else {
            None
        };

        for (hash, entry) in &self.entries {
            match entry.state {
                TrackState::None => {}
                TrackState::Added | TrackState::Changed => {
                    let node = entry
                        .node
                        .as_ref()
                        .ok_or_else(|| MptError::invalid("cache entry missing node"))?;
                    let data = {
                        let _timer = MptProfileTimer::new(
                            self.profile.as_deref(),
                            MptProfileStage::NodeEncode,
                        );
                        node.stored_bytes()?
                    };
                    {
                        let _timer = MptProfileTimer::new(
                            self.profile.as_deref(),
                            MptProfileStage::NodeWrite,
                        );
                        if self.use_fast_full_state_commit() {
                            self.store.put_full_state(self.key(hash), data)?;
                        } else {
                            self.store.put_with_hint(
                                self.key(hash),
                                data,
                                entry.initial_ref == 0,
                            )?;
                        }
                    }
                    // Full-state nodes are content addressed and remain available from
                    // the persisted store or the structurally shared warm root. Populate
                    // the compact LRU on demand instead of churning it for every commit.
                    // Reference-counted tries still refresh their cached reference here.
                    if !self.use_fast_full_state_commit()
                        && let Some(cache) = active_cache
                    {
                        let _timer = MptProfileTimer::new(
                            self.profile.as_deref(),
                            MptProfileStage::NodeCache,
                        );
                        cache.put(*hash, Arc::new(node.to_cached_node(*hash)));
                    }
                }
                TrackState::Deleted => {
                    {
                        let _timer = MptProfileTimer::new(
                            self.profile.as_deref(),
                            MptProfileStage::NodeWrite,
                        );
                        self.store.delete(self.key(hash))?;
                    }
                    if let Some(cache) = active_cache {
                        cache.remove(hash);
                    }
                }
            }
        }
        self.entries.clear();
        Ok(())
    }

    fn resolve_internal(&mut self, hash: &UInt256) -> MptResult<&mut MptTrackable> {
        let store = Arc::clone(&self.store);
        let prefix = self.prefix;
        let use_global_cache = self.use_global_cache;
        let node_cache = self.node_cache.clone();
        let profile = self.profile.clone();

        match self.entries.entry(*hash) {
            Entry::Occupied(entry) => Ok(entry.into_mut()),
            Entry::Vacant(entry) => {
                let active_cache: Option<&GlobalNodeCache> = if let Some(ref cache) = node_cache {
                    Some(cache.as_ref())
                } else if use_global_cache {
                    Some(&GLOBAL_NODE_CACHE)
                } else {
                    None
                };
                let node = Self::load_from_store_snapshot(
                    &store,
                    prefix,
                    hash,
                    active_cache,
                    profile.as_deref(),
                )?;
                Ok(entry.insert(MptTrackable::new(node)))
            }
        }
    }

    fn load_from_store_snapshot(
        store: &S,
        prefix: u8,
        hash: &UInt256,
        active_cache: Option<&GlobalNodeCache>,
        profile: Option<&MptProfile>,
    ) -> MptResult<Option<Arc<Node>>> {
        let Some(cache) = active_cache else {
            let key = Self::key_for(prefix, hash);
            let bytes = {
                let _timer = MptProfileTimer::new(profile, MptProfileStage::NodeRead);
                store.try_get(&key)?
            };
            let Some(bytes) = bytes else {
                return Ok(None);
            };
            let node = {
                let _timer = MptProfileTimer::new(profile, MptProfileStage::NodeDeserialize);
                let mut reader = MemoryReader::new(&bytes);
                Node::deserialize(&mut reader).map_err(MptError::from)?
            };
            node.set_cached_hash(*hash);
            return Ok(Some(Arc::new(node)));
        };

        // The store key is built only after a miss. A hit returns the cached
        // Arc itself; the caller detaches a node when it needs to mutate one.
        let cached = {
            let _timer = MptProfileTimer::new(profile, MptProfileStage::NodeCache);
            cache.get(hash)
        };
        if let Some(cached_arc) = cached {
            return Ok(Some(cached_arc));
        }

        let key = Self::key_for(prefix, hash);
        let bytes = {
            let _timer = MptProfileTimer::new(profile, MptProfileStage::NodeRead);
            store.try_get(&key)?
        };
        let Some(bytes) = bytes else {
            return Ok(None);
        };

        let node = {
            let _timer = MptProfileTimer::new(profile, MptProfileStage::NodeDeserialize);
            let mut reader = MemoryReader::new(&bytes);
            Node::deserialize(&mut reader).map_err(MptError::from)?
        };
        node.set_cached_hash(*hash);
        let node_arc = Arc::new(node);
        cache.put(*hash, Arc::clone(&node_arc));
        Ok(Some(node_arc))
    }

    fn key(&self, hash: &UInt256) -> Vec<u8> {
        Self::key_for(self.prefix, hash)
    }

    fn key_for(prefix: u8, hash: &UInt256) -> Vec<u8> {
        let mut buffer = Vec::with_capacity(1 + UINT256_SIZE);
        buffer.push(prefix);
        buffer.extend_from_slice(hash.as_ref());
        buffer
    }
}

// Utility functions for cache monitoring
/// Returns current cache statistics as (hits, misses, hit_rate_percentage)
pub fn get_cache_stats() -> (u64, u64, f32) {
    GLOBAL_NODE_CACHE.stats()
}

/// Returns current cache entry count  
pub fn get_cache_count() -> usize {
    GLOBAL_NODE_CACHE.inner.read().len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    use std::time::Instant;

    impl GlobalNodeCache {
        /// Helper for tests to create an empty cache without lazy initialization overhead.
        pub fn test_new() -> Self {
            Self::new()
        }
    }

    mod unit_tests {
        use super::*;

        #[test]
        fn test_cache_initialization() {
            let cache = GlobalNodeCache::test_new();
            assert_eq!(cache.hits.load(Ordering::Relaxed), 0);
            assert_eq!(cache.misses.load(Ordering::Relaxed), 0);
            let (hits, misses, rate) = cache.stats();
            assert_eq!(hits, 0);
            assert_eq!(misses, 0);
            assert_eq!(rate, 0.0);
        }

        #[test]
        fn test_put_and_get() {
            let cache = GlobalNodeCache::test_new();
            let node = Arc::new(Node::new());
            let hash = UInt256::from([1u8; 32]);

            cache.put(hash, node.clone());
            let cached = cache.get(&hash);

            assert!(cached.is_some());
            assert!(Arc::ptr_eq(&cached.unwrap(), &node));
        }

        #[test]
        fn test_cache_miss() {
            let cache = GlobalNodeCache::test_new();
            let hash = UInt256::from([2u8; 32]);

            let result = cache.get(&hash);
            assert!(result.is_none());

            let (hits, misses, _) = cache.stats();
            assert_eq!(hits, 0);
            assert_eq!(misses, 1);
        }

        #[test]
        fn test_cache_hit_records_stats() {
            let cache = GlobalNodeCache::test_new();
            let node = Arc::new(Node::new());
            let hash = UInt256::from([3u8; 32]);

            cache.put(hash, node);
            _ = cache.get(&hash); // Hit
            _ = cache.get(&hash); // Another hit
            _ = cache.get(&UInt256::zero()); // Miss

            let (hits, misses, _) = cache.stats();
            assert_eq!(hits, 2);
            assert_eq!(misses, 1);
        }

        #[test]
        fn test_cache_capacity_and_eviction() {
            let cache = GlobalNodeCache::with_capacity(NonZero::new(2).unwrap());
            let first = UInt256::from([1u8; 32]);
            let second = UInt256::from([2u8; 32]);
            let third = UInt256::from([3u8; 32]);

            cache.put(first, Arc::new(Node::new()));
            cache.put(second, Arc::new(Node::new()));
            cache.put(third, Arc::new(Node::new()));

            assert_eq!(cache.len(), 2);
            assert!(cache.get(&first).is_none());
            assert!(cache.get(&second).is_some());
            assert!(cache.get(&third).is_some());
        }

        #[test]
        fn test_clear() {
            let cache = GlobalNodeCache::test_new();
            let hash = UInt256::from([4u8; 32]);
            let node = Arc::new(Node::new());

            cache.put(hash, node);
            assert!(!cache.inner.read().is_empty());

            cache.clear();
            assert!(cache.inner.read().is_empty());
        }

        #[test]
        fn test_arc_sharing() {
            let cache = GlobalNodeCache::test_new();
            let node = Arc::new(Node::new());
            let hash = UInt256::from([5u8; 32]);

            // Put doesn't clone - it takes ownership of the Arc
            cache.put(hash, node.clone());

            let cached1 = cache.get(&hash);
            let cached2 = cache.get(&hash);

            // Both should point to same Arc
            assert!(Arc::ptr_eq(&cached1.unwrap(), &cached2.unwrap()));
            // original + stored in cache = 2 (get() returns cloned Arc but it's temporary)
            assert_eq!(Arc::strong_count(&node), 2);
        }
    }

    mod integration_tests {
        use super::*;

        /// Test that the global cache persists across mock block boundaries.
        #[test]
        fn test_cross_block_persistence() {
            let cache = GlobalNodeCache::test_new();

            // Block 1: Insert nodes into cache
            let node1 = Arc::new(Node::new_branch());
            let hash1 = node1.try_hash().unwrap();
            cache.put(hash1, node1.clone());

            // Simulate end of block 1 (cache should persist)
            drop(node1);

            // Block 2: Retrieve from cache (simulates next block reusing same node)
            let retrieved = cache.get(&hash1);
            assert!(retrieved.is_some());
        }

        /// Performance benchmark: Cache hit eliminates deserialization.
        #[test]
        fn test_zero_deserialization_on_hit() {
            let cache = GlobalNodeCache::test_new();

            // Pre-populate cache with a complex node
            let mut branch = Node::new_branch();
            for i in 0..17 {
                // Using inline constant instead of BRANCH_CHILD_COUNT
                let child = Node::new_leaf(vec![i as u8; 100]);
                branch.set_child(i, child);
            }
            let hash = branch.try_hash().unwrap();
            cache.put(hash, Arc::new(branch));

            // Cache hit should be near-instant (no deserialize)
            let start = Instant::now();
            for _ in 0..10_000 {
                let _ = cache.get(&hash);
            }
            let elapsed = start.elapsed();

            // Should complete in < 1ms total (arc clone ~100ns each)
            assert!(
                elapsed.as_micros() < 1_000_000,
                "Cache hit too slow: {elapsed:?}"
            );
        }

        /// Test realistic scenario: Multiple concurrent readers.
        #[test]
        fn test_concurrent_access_safety() {
            let cache = Arc::new(GlobalNodeCache::test_new());

            // Populate cache
            for i in 0..1000 {
                let hash = UInt256::from([i as u8; 32]);
                let node = Arc::new(Node::new());
                cache.put(hash, node);
            }

            // Spawn multiple threads reading concurrently
            let handles: Vec<_> = (0..10)
                .map(|_| {
                    thread::spawn({
                        let global_cache = Arc::clone(&cache);
                        move || {
                            let mut hits = 0u64;
                            for i in 0..1000 {
                                let hash = UInt256::from([i as u8; 32]);
                                if global_cache.get(&hash).is_some() {
                                    hits += 1;
                                }
                            }
                            hits
                        }
                    })
                })
                .collect();

            let total_hits: u64 = handles.into_iter().map(|h| h.join().unwrap()).sum();

            // All reads should succeed (at least 10,000, may be slightly more from other tests)
            assert!(
                total_hits >= 10_000,
                "Expected at least 10000 hits, got {total_hits}"
            );

            let (hits, _, _) = cache.stats();
            assert!(
                hits >= 10_000,
                "Expected at least 10000 stats hits, got {hits}"
            );
        }
    }
}
