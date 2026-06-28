//! # neo-state-service
//!
//! Canonical home for the Neo state-service plugin: state-root wire
//! types, persisted MPT storage, and the optional verification/cache
//! surfaces used by the StateService payload flow.
//!
//! ## Modules
//!
//! - [`state_root::StateRoot`] - a state-root snapshot for a single
//!   block.
//! - [`mpt_store::MptStore`] - persisted MPT-node + local-state-root
//!   storage (the C# `StateService` plugin's `Storage` layer) with the
//!   block-changeset application seam
//!   [`mpt_store::MptStore::apply_block_changes`].
//! - [`state_store::StateStore`] / [`state_store::StateStoreTransaction`]
//!   - verification-cache storage for locally known state roots and
//!   pending candidates.
//! - [`commit_handlers::StateServiceCommitHandlers`] - optional block
//!   lifecycle adapter over the local MPT state-root store.
//! - [`root_cache::StateRootCache`] and [`verification::Verifier`] -
//!   optional StateService payload verification helpers.
//!
//! ## Layering
//!
//! Sits in **Layer 3 (Domain services)**. Depends on:
//!
//! - Local wire-protocol modules (`Keys`, `MessageType`, `Vote`,
//!   `StateRootIngestStats`) for the state-service payload surface.
//! - `neo-storage` (Layer 1) - for the `DataCache` used during
//!   MPT computation.
//! - `neo-payloads` (Layer 2) - for the `ExtensiblePayload` used to
//!   transport state-root messages.
//! - `neo-payloads` (Layer 2) - for the typed block lifecycle handler traits.
//!
//! Must **not** depend on node-service, composition, plugin/RPC, or application
//! crates.

#![doc(html_root_url = "https://docs.rs/neo-state-service/0.8.0")]

pub mod commit_handlers;
pub mod keys;
pub mod message_type;
/// Lightweight state-root ingestion counters.
pub mod metrics;
pub mod mpt_store;
pub mod root_cache;
pub mod state_root;
pub mod state_store;
pub mod verification;
pub mod vote;

/// Extensible payload category for state service messages
/// (matches C# `StateService.StatePayloadCategory`).
pub const STATE_SERVICE_CATEGORY: &str = "StateService";

pub use keys::Keys;
pub use message_type::MessageType;
pub use metrics::{
    StateRootApplyMetrics, StateRootApplyStats, StateRootIngestMetrics, StateRootIngestStats,
};
pub use vote::Vote;

pub use mpt_store::{MptChange, MptReadSnapshot, MptStore};
pub use root_cache::{
    DEFAULT_ROOT_CACHE_CAPACITY, StateRootCache, StateRootCacheEntry, StateRootCacheStats,
    StateRootCacheStatsSnapshot,
};
pub use state_root::{CURRENT_VERSION, StateRoot};
pub use state_store::{StateStore, StateStoreLookup, StateStoreTransaction};
pub use verification::{StateRootCalculator, Verifier, VerifyOutcome};
