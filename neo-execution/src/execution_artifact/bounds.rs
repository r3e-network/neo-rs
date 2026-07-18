/// Hard bounds applied while retaining one comparison artifact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutionArtifactLimits {
    /// Maximum aggregate dynamic payload bytes retained by the artifact.
    pub max_retained_bytes: usize,
    /// Maximum stack-item roots across all artifact surfaces.
    pub max_stack_roots: usize,
    /// Maximum identity-bearing stack graph nodes.
    pub max_stack_nodes: usize,
    /// Maximum graph edges traversed from compound values.
    pub max_stack_edges: usize,
    /// Maximum recursive graph depth.
    pub max_stack_depth: usize,
    /// Maximum active invocation frames.
    pub max_invocation_frames: usize,
    /// Maximum storage changes across the engine and retained frame overlays.
    pub max_storage_changes: usize,
    /// Maximum point-read observations.
    pub max_storage_reads: usize,
    /// Maximum range-read observations.
    pub max_storage_ranges: usize,
    /// Maximum rows retained across range-read observations.
    pub max_storage_range_rows: usize,
    /// Maximum native-cache observations.
    pub max_native_cache_observations: usize,
    /// Maximum completed call observations.
    pub max_calls: usize,
    /// Maximum distinct logical invocation counters.
    pub max_invocation_counters: usize,
    /// Maximum witness observations.
    pub max_witnesses: usize,
    /// Maximum context observations.
    pub max_context_observations: usize,
    /// Maximum explicit fee-charge observations.
    pub max_fee_charges: usize,
    /// Maximum diagnostic callbacks.
    pub max_diagnostics: usize,
    /// Maximum notifications plus logs.
    pub max_events: usize,
    /// Maximum retained storage iterators.
    pub max_iterators: usize,
    /// Maximum rows retained across storage iterators.
    pub max_iterator_rows: usize,
}

impl ExecutionArtifactLimits {
    /// Conservative defaults for opt-in shadow replay.
    pub const DEFAULT: Self = Self {
        max_retained_bytes: 64 * 1024 * 1024,
        max_stack_roots: 65_536,
        max_stack_nodes: 262_144,
        max_stack_edges: 1_048_576,
        max_stack_depth: 1_024,
        max_invocation_frames: 1_024,
        max_storage_changes: 65_536,
        max_storage_reads: 65_536,
        max_storage_ranges: 4_096,
        max_storage_range_rows: 65_536,
        max_native_cache_observations: 16_384,
        max_calls: 16_384,
        max_invocation_counters: 16_384,
        max_witnesses: 16_384,
        max_context_observations: 65_536,
        max_fee_charges: 262_144,
        max_diagnostics: 262_144,
        max_events: 16_384,
        max_iterators: 4_096,
        max_iterator_rows: 65_536,
    };
}

impl Default for ExecutionArtifactLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Fail-closed artifact construction error.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ExecutionArtifactError {
    /// One configured artifact bound was exceeded.
    #[error("execution artifact {resource} requires {actual}, maximum {maximum}")]
    LimitExceeded {
        /// Bounded resource.
        resource: &'static str,
        /// Observed or requested amount.
        actual: usize,
        /// Configured maximum.
        maximum: usize,
    },
    /// A numeric field cannot be represented by the portable artifact format.
    #[error("execution artifact numeric field `{field}` does not fit u64")]
    NumericOverflow {
        /// Field that overflowed.
        field: &'static str,
    },
    /// A script container or persisting block could not produce its exact hash.
    #[error("execution artifact cannot hash {kind}: {message}")]
    InvalidHash {
        /// Hashed value kind.
        kind: &'static str,
        /// Underlying deterministic error.
        message: String,
    },
    /// A live callback could not be represented without changing execution.
    #[error("execution artifact cannot retain {kind}: {message}")]
    ObservationFailed {
        /// Semantic callback surface that could not be retained.
        kind: &'static str,
        /// Deterministic underlying failure.
        message: String,
    },
}
