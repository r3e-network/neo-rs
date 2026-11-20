//! Bounded, correctness-safe cache for repeated Merkle Patricia Trie proof
//! construction.
//!
//! # Why this module exists
//!
//! [`Trie::try_get_proof`](super::Trie::try_get_proof) walks from the root to
//! the requested leaf and re-serializes every visited node as it goes. Serving
//! the same proof repeatedly — a light client polling one storage key, or
//! several RPC callers asking for the same account proof — repeats that work
//! verbatim. This module adds an opt-in cache for the immutable proof bytes.
//!
//! No production caller of proof construction exists in this workspace yet:
//! proof generation is exercised by tests only. The cache is therefore an
//! opt-in reusable API, not a claimed node-throughput improvement.
//!
//! # Correctness rules
//!
//! * Entries are keyed by [`CommittedRoot`] (a finalized root hash), a
//!   caller-supplied store namespace, the proof format version and the full
//!   storage key. Two stores that expose the same root cannot collide.
//! * Only committed roots may be cached. [`Trie::get_proof_cached`] rejects a
//!   trie whose root is still dirty (mutable) and rejects a marker that does
//!   not match the trie's actual root, so a speculative root can never enter
//!   the cache.
//! * Values are immutable [`ProofBundle`]s shared through `Arc`. A bundle holds
//!   serialized node bytes only, never `Arc<Node>`, so the cache cannot pin MPT
//!   nodes or interfere with the node store's root retention policy.
//! * Identical concurrent requests are coalesced into one traversal and one
//!   serialization pass; the followers receive the leader's `Arc`.
//!
//! [`Trie::get_proof_cached`]: super::Trie::get_proof_cached

use super::error::{MptError, MptResult};
use neo_primitives::UInt256;
use parking_lot::{Condvar, Mutex};
use std::collections::{HashMap, HashSet, VecDeque};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Serialized proof-node format version guarded by [`ProofCacheKey`].
///
/// A change to the node encoding produced by
/// [`Node::to_array_without_reference`](super::Node::to_array_without_reference)
/// must bump this value so cached bundles can never be mistaken for a current
/// encoding.
pub const PROOF_FORMAT_VERSION: u16 = 1;

/// Approximate heap overhead of one proof node beyond its payload bytes:
/// the `Vec<u8>` header plus the allocator's bookkeeping.
const NODE_CONTAINER_OVERHEAD: usize = std::mem::size_of::<Vec<u8>>() + 16;

/// Approximate overhead of one cache entry beyond its node payload: the key
/// bytes, the map slot, the recency link and the `Arc` header.
const ENTRY_CONTAINER_OVERHEAD: usize = 128;

/// Marker asserting that a root hash is finalized and will not be mutated.
///
/// The only way to obtain a value is to state the claim explicitly, either
/// through [`Trie::committed_root`](super::Trie::committed_root) — which returns
/// `None` while the in-memory root is dirty — or through
/// [`CommittedRoot::from_committed`] when the caller already holds a persisted
/// root (for example a height-indexed state-root record).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CommittedRoot(UInt256);

impl CommittedRoot {
    /// Asserts that `root` is the finalized root of a committed trie.
    ///
    /// The assertion is unchecked: caching a proof under a root that is still
    /// being mutated would make the entry describe state that never existed.
    #[must_use]
    pub const fn from_committed(root: UInt256) -> Self {
        Self(root)
    }

    /// Returns the asserted root hash.
    #[must_use]
    pub const fn hash(&self) -> UInt256 {
        self.0
    }
}

/// Store or chain namespace disambiguating identical roots.
///
/// The same root hash can legitimately be resolved against distinct node
/// stores (for example a mainnet store and a test-chain store that replayed the
/// same writes). Entries from different namespaces must never be shared.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ProofCacheNamespace(u64);

impl ProofCacheNamespace {
    /// Creates a namespace from a caller-chosen identifier.
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// Returns the namespace identifier.
    #[must_use]
    pub const fn id(&self) -> u64 {
        self.0
    }
}

/// Immutable cache key: committed root, namespace, format version and full key.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProofCacheKey {
    root: UInt256,
    namespace: ProofCacheNamespace,
    format_version: u16,
    storage_key: Vec<u8>,
}

impl ProofCacheKey {
    /// Builds a key for `storage_key` under `committed` in `namespace`.
    #[must_use]
    pub fn new(
        committed: CommittedRoot,
        namespace: ProofCacheNamespace,
        storage_key: &[u8],
    ) -> Self {
        Self {
            root: committed.hash(),
            namespace,
            format_version: PROOF_FORMAT_VERSION,
            storage_key: storage_key.to_vec(),
        }
    }

    /// Returns the committed root hash this entry belongs to.
    #[must_use]
    pub const fn root(&self) -> UInt256 {
        self.root
    }

    /// Returns the store or chain namespace of this entry.
    #[must_use]
    pub const fn namespace(&self) -> ProofCacheNamespace {
        self.namespace
    }

    /// Returns the serialized proof-node format version of this entry.
    #[must_use]
    pub const fn format_version(&self) -> u16 {
        self.format_version
    }

    /// Returns the full storage key this proof was built for.
    #[must_use]
    pub fn storage_key(&self) -> &[u8] {
        &self.storage_key
    }

    /// Approximate bytes retained by this key inside the cache.
    fn retained_bytes(&self) -> usize {
        self.storage_key.len() + ENTRY_CONTAINER_OVERHEAD
    }
}

/// Immutable proof nodes shared by every caller that requests the same proof.
///
/// Nodes are stored in serialized form, sorted and de-duplicated so that the
/// byte accounting of an entry is stable. The bundle is content only: it holds
/// no reference into the trie or the node store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProofBundle {
    nodes: Arc<[Vec<u8>]>,
    byte_len: usize,
}

impl ProofBundle {
    /// Builds a bundle from collected proof nodes.
    fn from_nodes(mut nodes: Vec<Vec<u8>>) -> Self {
        nodes.sort_unstable();
        nodes.dedup();
        let byte_len = nodes.iter().map(|node| node.len()).sum();
        Self {
            nodes: Arc::from(nodes),
            byte_len,
        }
    }

    /// Returns the serialized proof nodes.
    #[must_use]
    pub fn nodes(&self) -> &[Vec<u8>] {
        &self.nodes
    }

    /// Returns the payload bytes of all proof nodes.
    #[must_use]
    pub const fn byte_len(&self) -> usize {
        self.byte_len
    }

    /// Returns the approximate bytes this bundle retains, including per-node
    /// container overhead.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        self.byte_len + self.nodes.len() * NODE_CONTAINER_OVERHEAD
    }

    /// Returns the number of proof nodes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns `true` when the bundle carries no nodes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Materializes the set accepted by [`Trie::verify_proof`](super::Trie::verify_proof).
    #[must_use]
    pub fn to_hash_set(&self) -> HashSet<Vec<u8>> {
        self.nodes.iter().cloned().collect()
    }
}

/// Capacity limits of a [`ProofCache`].
#[derive(Clone, Copy, Debug)]
pub struct ProofCacheConfig {
    max_entries: NonZeroUsize,
    max_bytes: NonZeroUsize,
    max_entry_bytes: NonZeroUsize,
}

impl ProofCacheConfig {
    /// Creates a configuration.
    ///
    /// `max_entry_bytes` is clamped to `max_bytes`: an admitted entry must
    /// always fit into an otherwise empty cache, which is what lets oversized
    /// proofs be rejected up front instead of evicting the working set.
    #[must_use]
    pub fn new(
        max_entries: NonZeroUsize,
        max_bytes: NonZeroUsize,
        max_entry_bytes: NonZeroUsize,
    ) -> Self {
        let max_entry_bytes = NonZeroUsize::new(max_entry_bytes.get().min(max_bytes.get()))
            .expect("clamped bound is non-zero because both inputs are non-zero");
        Self {
            max_entries,
            max_bytes,
            max_entry_bytes,
        }
    }

    /// Maximum number of retained proofs.
    #[must_use]
    pub const fn max_entries(&self) -> NonZeroUsize {
        self.max_entries
    }

    /// Byte budget of all retained proofs.
    #[must_use]
    pub const fn max_bytes(&self) -> NonZeroUsize {
        self.max_bytes
    }

    /// Largest proof, including key overhead, that may be admitted.
    #[must_use]
    pub const fn max_entry_bytes(&self) -> NonZeroUsize {
        self.max_entry_bytes
    }
}

impl Default for ProofCacheConfig {
    fn default() -> Self {
        Self::new(
            NonZeroUsize::new(256).expect("default entry bound"),
            NonZeroUsize::new(8 * 1024 * 1024).expect("default byte bound"),
            NonZeroUsize::new(256 * 1024).expect("default entry byte bound"),
        )
    }
}

/// Point-in-time counters and gauges of a [`ProofCache`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProofCacheStats {
    /// Requests served from the cache.
    pub hits: u64,
    /// Requests that required a fresh traversal or joined an in-flight one.
    pub misses: u64,
    /// Requests that joined an in-flight traversal instead of starting one.
    pub coalesced: u64,
    /// Proofs inserted into the cache.
    pub admissions: u64,
    /// Proofs built successfully but rejected because they exceeded
    /// [`ProofCacheConfig::max_entry_bytes`].
    pub oversized_rejects: u64,
    /// Entries removed to respect the entry or byte bound.
    pub evictions: u64,
    /// Proof traversals and serializations performed.
    pub constructions: u64,
    /// Entries currently retained.
    pub entries: usize,
    /// Approximate bytes currently retained.
    pub bytes: usize,
    /// Total wall-clock time spent constructing proofs.
    pub construction_time: Duration,
}

impl ProofCacheStats {
    /// Returns the mean construction latency, or `None` when nothing was built.
    #[must_use]
    pub fn mean_construction_time(&self) -> Option<Duration> {
        if self.constructions == 0 {
            return None;
        }
        Some(self.construction_time / u32::try_from(self.constructions).unwrap_or(u32::MAX))
    }

    /// Returns the hit rate as a percentage in `0.0..=100.0`.
    #[must_use]
    pub fn hit_rate_percent(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            return 0.0;
        }
        self.hits as f64 / total as f64 * 100.0
    }
}

#[derive(Debug, Default)]
struct Counters {
    hits: AtomicU64,
    misses: AtomicU64,
    coalesced: AtomicU64,
    admissions: AtomicU64,
    oversized: AtomicU64,
    evictions: AtomicU64,
    constructions: AtomicU64,
    construction_nanos: AtomicU64,
}

impl Counters {
    fn bump(counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::Relaxed);
    }

    fn add(counter: &AtomicU64, value: u64) {
        counter.fetch_add(value, Ordering::Relaxed);
    }
}

struct CacheEntry {
    bundle: Arc<ProofBundle>,
    bytes: usize,
    tick: u64,
}

/// Outcome published by the thread that performed a traversal.
#[derive(Clone)]
enum FlightOutcome {
    Ready(Arc<ProofBundle>),
    Missing,
}

enum FlightState {
    Pending,
    Settled(FlightOutcome),
    Failed,
}

/// Single-flight slot that lets identical concurrent requests share one build.
struct Flight {
    state: Mutex<FlightState>,
    settled: Condvar,
}

impl Flight {
    fn new() -> Self {
        Self {
            state: Mutex::new(FlightState::Pending),
            settled: Condvar::new(),
        }
    }

    /// Blocks until the leader publishes an outcome.
    ///
    /// Returns `None` when the leader failed, in which case the waiter retries
    /// so a transient error cannot be served as a cached answer.
    fn wait(&self) -> Option<FlightOutcome> {
        let mut state = self.state.lock();
        loop {
            match &*state {
                FlightState::Pending => self.settled.wait(&mut state),
                FlightState::Settled(outcome) => return Some(outcome.clone()),
                FlightState::Failed => return None,
            }
        }
    }

    fn settle(&self, outcome: FlightOutcome) {
        *self.state.lock() = FlightState::Settled(outcome);
        self.settled.notify_all();
    }

    fn fail(&self) {
        *self.state.lock() = FlightState::Failed;
        self.settled.notify_all();
    }
}

/// Releases a leader's flight even when the traversal panics, so waiters are
/// woken and retry instead of blocking forever on a flight nobody will settle.
struct FlightGuard<'a> {
    cache: &'a ProofCache,
    key: &'a ProofCacheKey,
    armed: bool,
}

impl<'a> FlightGuard<'a> {
    fn new(cache: &'a ProofCache, key: &'a ProofCacheKey) -> Self {
        Self {
            cache,
            key,
            armed: true,
        }
    }

    /// Marks the flight as settled by the caller.
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for FlightGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.cache.fail_flight(self.key);
        }
    }
}

struct Inner {
    entries: HashMap<ProofCacheKey, CacheEntry>,
    /// `(tick, key)` recency log; stale ticks are skipped during eviction.
    recency: VecDeque<(u64, ProofCacheKey)>,
    flights: HashMap<ProofCacheKey, Arc<Flight>>,
    bytes: usize,
    tick: u64,
}

impl Inner {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            recency: VecDeque::new(),
            flights: HashMap::new(),
            bytes: 0,
            tick: 0,
        }
    }

    fn next_tick(&mut self) -> u64 {
        self.tick += 1;
        self.tick
    }

    /// Returns the cached bundle and refreshes the entry's recency.
    fn lookup(&mut self, key: &ProofCacheKey) -> Option<Arc<ProofBundle>> {
        let tick = self.next_tick();
        let entry = self.entries.get_mut(key)?;
        entry.tick = tick;
        let bundle = Arc::clone(&entry.bundle);
        self.recency.push_back((tick, key.clone()));
        self.compact_recency();
        Some(bundle)
    }

    /// Inserts one entry and evicts until both bounds hold again.
    fn admit(&mut self, key: ProofCacheKey, bundle: Arc<ProofBundle>, entry_bytes: usize) {
        let tick = self.next_tick();
        if let Some(previous) = self.entries.insert(
            key.clone(),
            CacheEntry {
                bundle,
                bytes: entry_bytes,
                tick,
            },
        ) {
            self.bytes = self.bytes.saturating_sub(previous.bytes);
        }
        self.bytes += entry_bytes;
        self.recency.push_back((tick, key));
    }

    /// Evicts least-recently-used entries until both bounds are satisfied.
    ///
    /// The freshly admitted entry is the most recent one, so it is the last
    /// candidate; because an admitted entry always fits the byte bound by its
    /// own, the loop cannot evict it.
    fn enforce_bounds(&mut self, config: &ProofCacheConfig) -> u64 {
        let mut evicted = 0;
        while self.bytes > config.max_bytes.get() || self.entries.len() > config.max_entries.get() {
            let Some((tick, key)) = self.recency.pop_front() else {
                break;
            };
            let is_current = self
                .entries
                .get(&key)
                .is_some_and(|entry| entry.tick == tick);
            if !is_current {
                continue;
            }
            if let Some(entry) = self.entries.remove(&key) {
                self.bytes = self.bytes.saturating_sub(entry.bytes);
                evicted += 1;
            }
        }
        evicted
    }

    /// Keeps the recency log proportional to the retained entries.
    fn compact_recency(&mut self) {
        let limit = self.entries.len().saturating_mul(4).saturating_add(8);
        if self.recency.len() <= limit {
            return;
        }
        let mut live: Vec<(u64, ProofCacheKey)> = self
            .entries
            .iter()
            .map(|(key, entry)| (entry.tick, key.clone()))
            .collect();
        live.sort_unstable_by_key(|(tick, _)| *tick);
        self.recency = live.into();
    }
}

/// Byte- and entry-bounded LRU cache of constructed MPT proofs.
///
/// The cache is safe to share between threads; identical concurrent requests
/// are served by a single traversal.
pub struct ProofCache {
    config: ProofCacheConfig,
    inner: Mutex<Inner>,
    counters: Counters,
}

impl ProofCache {
    /// Creates an empty cache with the supplied bounds.
    #[must_use]
    pub fn new(config: ProofCacheConfig) -> Self {
        Self {
            config,
            inner: Mutex::new(Inner::new()),
            counters: Counters::default(),
        }
    }

    /// Creates an empty cache with [`ProofCacheConfig::default`].
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(ProofCacheConfig::default())
    }

    /// Returns the configured bounds.
    #[must_use]
    pub const fn config(&self) -> &ProofCacheConfig {
        &self.config
    }

    /// Returns the cached bundle for `key`, counting a hit or a miss.
    #[must_use]
    pub fn get(&self, key: &ProofCacheKey) -> Option<Arc<ProofBundle>> {
        let mut inner = self.inner.lock();
        match inner.lookup(key) {
            Some(bundle) => {
                Counters::bump(&self.counters.hits);
                Some(bundle)
            }
            None => {
                Counters::bump(&self.counters.misses);
                None
            }
        }
    }

    /// Returns `true` when `key` is retained without changing its recency.
    #[must_use]
    pub fn contains(&self, key: &ProofCacheKey) -> bool {
        self.inner.lock().entries.contains_key(key)
    }

    /// Returns the number of retained proofs.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.lock().entries.len()
    }

    /// Returns `true` when no proof is retained.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the approximate bytes retained by all entries.
    #[must_use]
    pub fn byte_len(&self) -> usize {
        self.inner.lock().bytes
    }

    /// Drops every retained entry. In-flight builds are unaffected.
    pub fn clear(&self) {
        let mut inner = self.inner.lock();
        inner.entries.clear();
        inner.recency.clear();
        inner.bytes = 0;
    }

    /// Returns a point-in-time snapshot of counters and gauges.
    #[must_use]
    pub fn stats(&self) -> ProofCacheStats {
        let (entries, bytes) = {
            let inner = self.inner.lock();
            (inner.entries.len(), inner.bytes)
        };
        ProofCacheStats {
            hits: self.counters.hits.load(Ordering::Relaxed),
            misses: self.counters.misses.load(Ordering::Relaxed),
            coalesced: self.counters.coalesced.load(Ordering::Relaxed),
            admissions: self.counters.admissions.load(Ordering::Relaxed),
            oversized_rejects: self.counters.oversized.load(Ordering::Relaxed),
            evictions: self.counters.evictions.load(Ordering::Relaxed),
            constructions: self.counters.constructions.load(Ordering::Relaxed),
            entries,
            bytes,
            construction_time: Duration::from_nanos(
                self.counters.construction_nanos.load(Ordering::Relaxed),
            ),
        }
    }

    /// Returns a cached bundle or builds it exactly once.
    ///
    /// `build` returns `Ok(Some(nodes))` for a proof, `Ok(None)` when the key is
    /// absent from the trie, and `Err` for a traversal failure. Absent keys and
    /// errors are never cached: the negative outcome stays a property of the
    /// trie, and a later call re-reads it.
    ///
    /// The closure only runs on the thread that wins the traversal, so it does
    /// not need to be `Send`; concurrent callers block on the winner's result.
    pub fn get_or_construct<F>(
        &self,
        key: &ProofCacheKey,
        build: F,
    ) -> MptResult<Option<Arc<ProofBundle>>>
    where
        F: FnOnce() -> MptResult<Option<Vec<Vec<u8>>>>,
    {
        let mut build = Some(build);
        loop {
            let flight = {
                let mut inner = self.inner.lock();
                if let Some(bundle) = inner.lookup(key) {
                    Counters::bump(&self.counters.hits);
                    return Ok(Some(bundle));
                }
                Counters::bump(&self.counters.misses);
                match inner.flights.get(key) {
                    Some(existing) => {
                        Counters::bump(&self.counters.coalesced);
                        Some(Arc::clone(existing))
                    }
                    None => {
                        let created = Arc::new(Flight::new());
                        inner.flights.insert(key.clone(), Arc::clone(&created));
                        // `None` marks this caller as the leader.
                        None
                    }
                }
            };

            if let Some(flight) = flight {
                match flight.wait() {
                    Some(FlightOutcome::Ready(bundle)) => return Ok(Some(bundle)),
                    Some(FlightOutcome::Missing) => return Ok(None),
                    // The leader failed; retry as a fresh request so the error
                    // is not shared as a cached answer.
                    None => continue,
                }
            }

            let builder = build
                .take()
                .expect("the leader runs the build closure at most once");
            let mut guard = FlightGuard::new(self, key);
            let started = Instant::now();
            let outcome = builder();
            let elapsed = started.elapsed();
            Counters::bump(&self.counters.constructions);
            Counters::add(
                &self.counters.construction_nanos,
                u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX),
            );

            match outcome {
                Ok(Some(nodes)) => {
                    let bundle = Arc::new(ProofBundle::from_nodes(nodes));
                    self.publish(key, &bundle);
                    guard.disarm();
                    return Ok(Some(bundle));
                }
                Ok(None) => {
                    self.finish_flight(key, None);
                    guard.disarm();
                    return Ok(None);
                }
                Err(error) => {
                    self.fail_flight(key);
                    guard.disarm();
                    return Err(error);
                }
            }
        }
    }

    /// Admits a built bundle, then releases waiters.
    ///
    /// Admission happens before the flight is settled so that a request
    /// arriving in between observes the cache entry rather than starting a
    /// duplicate traversal.
    fn publish(&self, key: &ProofCacheKey, bundle: &Arc<ProofBundle>) {
        let entry_bytes = bundle.retained_bytes() + key.retained_bytes();
        let admitted = entry_bytes <= self.config.max_entry_bytes.get();

        if admitted {
            let mut inner = self.inner.lock();
            inner.admit(key.clone(), Arc::clone(bundle), entry_bytes);
            let evicted = inner.enforce_bounds(&self.config);
            drop(inner);
            Counters::bump(&self.counters.admissions);
            Counters::add(&self.counters.evictions, evicted);
        } else {
            // Rejecting an oversized proof must not touch the working set.
            Counters::bump(&self.counters.oversized);
        }

        self.finish_flight(key, Some(FlightOutcome::Ready(Arc::clone(bundle))));
    }

    fn fail_flight(&self, key: &ProofCacheKey) {
        let flight = self.inner.lock().flights.remove(key);
        if let Some(flight) = flight {
            flight.fail();
        }
    }

    fn finish_flight(&self, key: &ProofCacheKey, outcome: Option<FlightOutcome>) {
        let flight = self.inner.lock().flights.remove(key);
        let Some(flight) = flight else {
            return;
        };
        match outcome {
            Some(outcome) => flight.settle(outcome),
            None => flight.settle(FlightOutcome::Missing),
        }
    }
}

impl std::fmt::Debug for ProofCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let stats = self.stats();
        f.debug_struct("ProofCache")
            .field("config", &self.config)
            .field("entries", &stats.entries)
            .field("bytes", &stats.bytes)
            .finish()
    }
}

/// Error for a proof request that violates the committed-root precondition.
pub(crate) fn speculative_root_error(detail: impl std::fmt::Display) -> MptError {
    MptError::invalid(format!("proof cache rejection: {detail}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mpt_trie::Trie;
    use crate::mpt_trie::tests::mpt_tests::MockStore;
    use std::sync::Barrier;
    use std::sync::atomic::AtomicUsize;
    use std::thread;

    const NAMESPACE: ProofCacheNamespace = ProofCacheNamespace::new(0x4e45_4f00);

    /// Commits `entries` into a fresh full-state trie, retaining every root.
    fn build_trie(
        entries: &[(&[u8], Vec<u8>)],
    ) -> (Arc<MockStore>, Trie<MockStore>, CommittedRoot) {
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
        let (_store, mut trie, root) =
            build_trie(&[(b"one", b"1".to_vec()), (b"two", b"2".to_vec())]);
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
}
