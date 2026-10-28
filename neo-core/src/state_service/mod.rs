//! State Service Module
//!
//! This module provides the state root computation and verification service
//! that matches the C# StateService plugin exactly.

pub mod async_state_root;
#[cfg(feature = "runtime")]
pub(crate) mod commit_handlers;
#[cfg(feature = "runtime")]
pub mod conflict_detector;
pub mod keys;
pub mod message_type;
/// Metrics counters for state root ingestion from the network.
pub mod metrics;
#[cfg(feature = "runtime")]
pub mod optimistic_executor;
#[cfg(feature = "runtime")]
pub mod parallel_execution_integration;
#[cfg(feature = "runtime")]
pub mod prefetch_pipeline;
// WIP: the prefetch subsystem (background worker, BlockPrefetchService, pipeline
// demo) is not yet implemented; gate it behind the `prefetch` feature (not in
// default) so the default lib still builds without inventing the missing subsystem.
#[cfg(feature = "prefetch")]
pub mod pipeline_integration_example;
#[cfg(feature = "prefetch")]
pub mod prefetch_background;
#[cfg(feature = "prefetch")]
pub mod prefetch_worker;
pub mod read_only_store;
pub mod root_cache;
pub mod state_root;
pub mod state_store;
pub mod tiered_storage;
#[cfg(feature = "runtime")]
pub mod verification;
pub mod vote;

/// Extensible payload category for state service messages (matches C# StateService.StatePayloadCategory).
pub const STATE_SERVICE_CATEGORY: &str = "StateService";

pub use keys::Keys;
pub use message_type::MessageType;
pub use metrics::StateRootIngestStats;
pub use root_cache::{
    DEFAULT_ROOT_CACHE_CAPACITY, StateRootCache, StateRootCacheEntry, StateRootCacheStats,
};
pub use state_root::StateRoot;
pub use state_store::{StateRootDivergence, StateStore, StateStoreTransaction};
pub use vote::Vote;

/// Event published when a validated state root is persisted.
#[derive(Debug, Clone)]
pub struct ValidatedRootPersisted {
    /// Block index (height) of the validated state root that was persisted.
    pub index: u32,
}
