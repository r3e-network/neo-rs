use neo_system::ExecutionProfileSnapshot;

/// Direct-dispatch outcome buckets. Exactly two values, so the label is bounded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FastPathOutcome {
    /// Direct dispatch completed successfully.
    Halt,
    /// Direct dispatch ended in a canonical `FAULT`.
    Fault,
}

impl FastPathOutcome {
    /// Every exported outcome, in exposition order.
    pub const ALL: [Self; 2] = [Self::Halt, Self::Fault];

    /// Label value for `neo_fast_path_direct_outcomes_total`.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Halt => "halt",
            Self::Fault => "fault",
        }
    }
}

/// Read-cache inserts rejected by the admission checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectedInsert {
    /// The entry alone exceeded the per-entry share of the byte budget.
    Oversized,
    /// The byte budget stayed exhausted after eviction.
    Budget,
}

impl RejectedInsert {
    /// Every exported rejection reason, in exposition order.
    pub const ALL: [Self; 2] = [Self::Oversized, Self::Budget];

    /// Label value for `neo_storage_read_cache_rejected_total`.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Oversized => "oversized",
            Self::Budget => "budget",
        }
    }
}

/// MPT persistence stages with a dedicated timer in `neo-system`.
///
/// The stage set is fixed by `ExecutionStats`, so the `stage` label is bounded
/// by construction. These timings only accumulate while persistence profiling
/// is enabled; `neo_execution_profiling_enabled` reports whether they are live.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MptStage {
    /// Whole-trie update.
    Update,
    /// Trie commit into the node store.
    Commit,
    /// Root hash computation.
    RootHash,
    /// Node hashing (SHA-256 over encoded nodes).
    Hash,
    /// Node serialization before hashing.
    Serialization,
    /// Double SHA-256 hashing.
    Hash256,
    /// Node cache lookups.
    NodeCache,
    /// Node reads from the backing store.
    NodeRead,
    /// Node deserialization.
    NodeDeserialize,
    /// Node encoding.
    NodeEncode,
    /// Node writes.
    NodeWrite,
}

/// Number of exported MPT stages. Bounded by construction.
pub const MPT_STAGE_COUNT: usize = 11;

impl MptStage {
    /// Every exported stage, in exposition order.
    pub const ALL: [Self; MPT_STAGE_COUNT] = [
        Self::Update,
        Self::Commit,
        Self::RootHash,
        Self::Hash,
        Self::Serialization,
        Self::Hash256,
        Self::NodeCache,
        Self::NodeRead,
        Self::NodeDeserialize,
        Self::NodeEncode,
        Self::NodeWrite,
    ];

    /// Label value for `neo_mpt_stage_seconds_total`.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Update => "update",
            Self::Commit => "commit",
            Self::RootHash => "root_hash",
            Self::Hash => "hash",
            Self::Serialization => "serialization",
            Self::Hash256 => "hash256",
            Self::NodeCache => "node_cache",
            Self::NodeRead => "node_read",
            Self::NodeDeserialize => "node_deserialize",
            Self::NodeEncode => "node_encode",
            Self::NodeWrite => "node_write",
        }
    }

    pub(super) const fn index(self) -> usize {
        match self {
            Self::Update => 0,
            Self::Commit => 1,
            Self::RootHash => 2,
            Self::Hash => 3,
            Self::Serialization => 4,
            Self::Hash256 => 5,
            Self::NodeCache => 6,
            Self::NodeRead => 7,
            Self::NodeDeserialize => 8,
            Self::NodeEncode => 9,
            Self::NodeWrite => 10,
        }
    }

    /// Reads this stage's accumulated wall-clock time.
    pub fn seconds(self, profile: &ExecutionProfileSnapshot) -> f64 {
        match self {
            Self::Update => profile.mpt_update.as_secs_f64(),
            Self::Commit => profile.mpt_commit.as_secs_f64(),
            Self::RootHash => profile.mpt_root_hash.as_secs_f64(),
            Self::Hash => profile.mpt_hash.as_secs_f64(),
            Self::Serialization => profile.mpt_serialization.as_secs_f64(),
            Self::Hash256 => profile.mpt_hash256.as_secs_f64(),
            Self::NodeCache => profile.mpt_node_cache.as_secs_f64(),
            Self::NodeRead => profile.mpt_node_read.as_secs_f64(),
            Self::NodeDeserialize => profile.mpt_node_deserialize.as_secs_f64(),
            Self::NodeEncode => profile.mpt_node_encode.as_secs_f64(),
            Self::NodeWrite => profile.mpt_node_write.as_secs_f64(),
        }
    }
}

/// Block-level native hook phases timed by the persistence profiler.
///
/// The profiler records one duration per phase for each persisted block, so the
/// `phase` label is bounded by construction. Per-contract hook timings have no
/// production data source and are deliberately not exported.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeHookPhase {
    /// `OnPersist` dispatch for the block.
    OnPersist,
    /// `PostPersist` dispatch for the block.
    PostPersist,
}

/// Number of exported native hook phases. Bounded by construction.
pub const NATIVE_HOOK_PHASE_COUNT: usize = 2;

impl NativeHookPhase {
    /// Every exported phase, in exposition order.
    pub const ALL: [Self; NATIVE_HOOK_PHASE_COUNT] = [Self::OnPersist, Self::PostPersist];

    /// Label value for `neo_native_hook_seconds_total`.
    pub const fn label(self) -> &'static str {
        match self {
            Self::OnPersist => "on_persist",
            Self::PostPersist => "post_persist",
        }
    }

    pub(super) const fn index(self) -> usize {
        match self {
            Self::OnPersist => 0,
            Self::PostPersist => 1,
        }
    }
}
