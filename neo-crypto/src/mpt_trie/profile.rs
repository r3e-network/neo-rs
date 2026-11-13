//! Optional wall-clock observation for Merkle Patricia trie operations.
//!
//! Profiling is diagnostic only: it never selects nodes, changes commit order,
//! or alters error handling. Disabled timers do not read the clock.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Observable MPT stages.
///
/// `Update`, `Commit`, and `RootHash` are inclusive operation timers. `Hash`
/// is nested in commit or root hashing; `Serialization` and `Hash256` are
/// nested in hashing. Node cache, storage, and codec timers can be nested in
/// update or commit. These durations must not be summed as disjoint work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MptProfileStage {
    /// Inclusive put or delete traversal and mutation.
    Update,
    /// Inclusive trie flush and node-cache commit.
    Commit,
    /// Inclusive root-hash request.
    RootHash,
    /// Inclusive dirty-subtree hashing pass.
    Hash,
    /// Node-body serialization performed for hashing.
    Serialization,
    /// Cryptographic Hash256 over a serialized node body.
    Hash256,
    /// Optional node-cache lookup.
    NodeCache,
    /// Persistent or block-snapshot node read.
    NodeRead,
    /// Persistent node decoding.
    NodeDeserialize,
    /// Stored-node encoding before a write.
    NodeEncode,
    /// Persistent or block-snapshot node write or deletion.
    NodeWrite,
}

const PROFILE_UPDATE: usize = 0;
const PROFILE_COMMIT: usize = 1;
const PROFILE_ROOT_HASH: usize = 2;
const PROFILE_HASH: usize = 3;
const PROFILE_SERIALIZATION: usize = 4;
const PROFILE_HASH256: usize = 5;
const PROFILE_NODE_CACHE: usize = 6;
const PROFILE_NODE_READ: usize = 7;
const PROFILE_NODE_DESERIALIZE: usize = 8;
const PROFILE_NODE_ENCODE: usize = 9;
const PROFILE_NODE_WRITE: usize = 10;
const PROFILE_STAGE_COUNT: usize = 11;

/// Aggregated MPT timings for one state-root computation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MptProfileSnapshot {
    /// Inclusive put or delete traversal and mutation time.
    pub update: Duration,
    /// Inclusive trie flush and node-cache commit time.
    pub commit: Duration,
    /// Inclusive root-hash request time.
    pub root_hash: Duration,
    /// Inclusive dirty-subtree hashing time.
    pub hash: Duration,
    /// Node-body serialization time for hashing.
    pub serialization: Duration,
    /// Cryptographic Hash256 time.
    pub hash256: Duration,
    /// Optional node-cache lookup time.
    pub node_cache: Duration,
    /// Persistent or block-snapshot node-read time.
    pub node_read: Duration,
    /// Persistent node-decoding time.
    pub node_deserialize: Duration,
    /// Stored-node encoding time.
    pub node_encode: Duration,
    /// Persistent or block-snapshot node-write time.
    pub node_write: Duration,
}

/// Process-local observability counters shared with trie worker threads.
#[derive(Debug, Default)]
pub struct MptProfile {
    nanos: [AtomicU64; PROFILE_STAGE_COUNT],
}

impl MptProfile {
    /// Returns an observability snapshot; these values are never consensus state.
    #[must_use]
    pub fn snapshot(&self) -> MptProfileSnapshot {
        let duration =
            |index: usize| Duration::from_nanos(self.nanos[index].load(Ordering::Relaxed));
        MptProfileSnapshot {
            update: duration(PROFILE_UPDATE),
            commit: duration(PROFILE_COMMIT),
            root_hash: duration(PROFILE_ROOT_HASH),
            hash: duration(PROFILE_HASH),
            serialization: duration(PROFILE_SERIALIZATION),
            hash256: duration(PROFILE_HASH256),
            node_cache: duration(PROFILE_NODE_CACHE),
            node_read: duration(PROFILE_NODE_READ),
            node_deserialize: duration(PROFILE_NODE_DESERIALIZE),
            node_encode: duration(PROFILE_NODE_ENCODE),
            node_write: duration(PROFILE_NODE_WRITE),
        }
    }

    fn index(stage: MptProfileStage) -> usize {
        match stage {
            MptProfileStage::Update => PROFILE_UPDATE,
            MptProfileStage::Commit => PROFILE_COMMIT,
            MptProfileStage::RootHash => PROFILE_ROOT_HASH,
            MptProfileStage::Hash => PROFILE_HASH,
            MptProfileStage::Serialization => PROFILE_SERIALIZATION,
            MptProfileStage::Hash256 => PROFILE_HASH256,
            MptProfileStage::NodeCache => PROFILE_NODE_CACHE,
            MptProfileStage::NodeRead => PROFILE_NODE_READ,
            MptProfileStage::NodeDeserialize => PROFILE_NODE_DESERIALIZE,
            MptProfileStage::NodeEncode => PROFILE_NODE_ENCODE,
            MptProfileStage::NodeWrite => PROFILE_NODE_WRITE,
        }
    }

    fn record(&self, stage: MptProfileStage, elapsed: Duration) {
        let nanos = elapsed.as_nanos().min(u128::from(u64::MAX)) as u64;
        self.nanos[Self::index(stage)].fetch_add(nanos, Ordering::Relaxed);
    }
}

/// Records its stage on drop, including early-error exits from a profiled scope.
pub(crate) struct MptProfileTimer<'a> {
    profile: Option<&'a MptProfile>,
    stage: MptProfileStage,
    started: Option<Instant>,
}

impl<'a> MptProfileTimer<'a> {
    pub(crate) fn new(profile: Option<&'a MptProfile>, stage: MptProfileStage) -> Self {
        Self {
            profile,
            stage,
            started: profile.map(|_| Instant::now()),
        }
    }
}

impl Drop for MptProfileTimer<'_> {
    fn drop(&mut self) {
        if let (Some(profile), Some(started)) = (self.profile, self.started) {
            profile.record(self.stage, started.elapsed());
        }
    }
}
