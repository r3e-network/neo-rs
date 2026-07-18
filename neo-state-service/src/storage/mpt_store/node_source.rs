//! Authoritative read snapshots for physically separated MPT node storage.
//!
//! StateService root/index metadata remains in its ordinary storage backing,
//! while exact `0xf0 || node_hash` bytes may live in a separately published
//! node pack. The factory returns an already-pinned immutable generation so a
//! trie walk cannot observe compaction or a later commit halfway through.

use neo_storage::StorageResult;
use std::sync::Arc;

/// One immutable node generation plus its publication sequence.
pub struct MptNodeReadGeneration {
    sequence: u64,
    snapshot: Arc<dyn MptNodeReadSnapshot>,
}

impl MptNodeReadGeneration {
    /// Constructs a pinned generation. Sequences must change whenever the
    /// factory publishes a different node view.
    pub fn new(sequence: u64, snapshot: Arc<dyn MptNodeReadSnapshot>) -> Self {
        Self { sequence, snapshot }
    }

    /// Publication sequence captured with the snapshot.
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Immutable node snapshot belonging to this sequence.
    pub fn snapshot(&self) -> Arc<dyn MptNodeReadSnapshot> {
        Arc::clone(&self.snapshot)
    }
}

/// Immutable authoritative view of the MPT node namespace.
///
/// Implementations must distinguish absence from backend failure. A missing
/// key is authoritative and callers must not fall back to an older MDBX node
/// namespace.
pub trait MptNodeReadSnapshot: Send + Sync + 'static {
    /// Reads one exact prefixed node key.
    fn try_get_node_bytes(&self, key: &[u8]) -> StorageResult<Option<Vec<u8>>>;

    /// Reads sorted exact prefixed node keys, preserving input order and
    /// duplicates.
    fn try_get_node_bytes_sorted(&self, keys: &[&[u8]]) -> StorageResult<Vec<Option<Vec<u8>>>> {
        keys.iter()
            .map(|key| self.try_get_node_bytes(key))
            .collect()
    }
}

/// Infallible provider of the currently published immutable node generation.
///
/// Opening and validating the physical backend is deliberately outside this
/// trait: node composition must fail startup before constructing StateService
/// if the committed generation cannot be pinned.
pub trait MptNodeSnapshotFactory: Send + Sync + 'static {
    /// Pins the generation paired with the currently visible StateService
    /// metadata commit.
    fn snapshot(&self) -> Arc<dyn MptNodeReadSnapshot>;

    /// Pins the current node generation and its publication sequence.
    ///
    /// Static factories may use the default sequence. Runtime authorities
    /// must override this together with [`Self::is_generation_current`].
    fn pinned_generation(&self) -> MptNodeReadGeneration {
        MptNodeReadGeneration::new(0, self.snapshot())
    }

    /// Returns whether `sequence` still names the current published node
    /// generation. Runtime authorities hold their publication lock across
    /// the MDBX marker commit and pointer swap, so this validation closes the
    /// old-node/new-metadata race without holding a lock during trie reads.
    fn is_generation_current(&self, sequence: u64) -> bool {
        sequence == 0
    }
}
