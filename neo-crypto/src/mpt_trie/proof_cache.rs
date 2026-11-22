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

/// Error for a proof request that violates the committed-root precondition.
pub(crate) fn speculative_root_error(detail: impl std::fmt::Display) -> MptError {
    MptError::invalid(format!("proof cache rejection: {detail}"))
}

mod core;
mod flight;

pub use core::ProofCache;

#[cfg(test)]
mod tests;
