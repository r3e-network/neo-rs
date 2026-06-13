//! # neo-state-service
//!
//! Canonical home for the Neo state-service plugin: state roots,
//! state-root cache, state store, commit-handler pipeline, and
//! verification pipeline.
//!
//! ## Modules
//!
//! - [`state_root::StateRoot`] - a state-root snapshot for a single
//!   block.
//! - [`root_cache::StateRootCache`] - LRU cache of recently-validated
//!   state roots.
//! - [`state_store::StateStore`] / [`state_store::StateStoreTransaction`]
//!   - storage for state roots and pending candidates.
//! - [`mpt_store::MptStore`] - persisted MPT-node + local-state-root
//!   storage (the C# `StateService` plugin's `Storage` layer) with the
//!   block-changeset application seam
//!   [`mpt_store::MptStore::apply_block_changes`].
//! - [`commit_handlers::StateServiceCommitHandlers`] - block-commit
//!   handler pipeline (computes and stages state roots).
//! - [`verification::Verifier`] - state-root verification pipeline.
//!
//! ## Layering
//!
//! Sits in **Layer 2 (service)**. Depends on:
//!
//! - Local wire-protocol modules (`Keys`, `MessageType`, `Vote`,
//!   `StateRootIngestStats`) for the state-service payload surface.
//! - `neo-storage` (Layer 1) - for the `DataCache` used during
//!   MPT computation.
//! - `neo-payloads` (Layer 1) - for the `ExtensiblePayload` used to
//!   transport state-root messages.
//! - `neo-payloads` (Layer 1) - for the typed block lifecycle handler traits.
//!
//! Must **not** depend on `neo-core` (deleted).

#![doc(html_root_url = "https://docs.rs/neo-state-service/0.7.2")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

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
pub use metrics::{StateRootIngestStats, record_ingest_result, state_root_ingest_stats};
pub use vote::Vote;

pub use mpt_store::{MptChange, MptReadSnapshot, MptStore};
pub use root_cache::{
    DEFAULT_ROOT_CACHE_CAPACITY, StateRootCache, StateRootCacheEntry, StateRootCacheStats,
    StateRootCacheStatsSnapshot,
};
pub use state_root::{CURRENT_VERSION, StateRoot};
pub use state_store::{StateStore, StateStoreLookup, StateStoreTransaction};
