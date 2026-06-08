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
//! - [`commit_handlers::StateServiceCommitHandlers`] - block-commit
//!   handler pipeline (computes and stages state roots).
//! - [`verification::Verifier`] - state-root verification pipeline.
//!
//! ## Layering
//!
//! Sits in **Layer 2 (service)**. Depends on:
//!
//! - `neo-state-types` (Layer 1) - for the wire-protocol enums
//!   (`Keys`, `MessageType`, `Vote`, `StateRootIngestStats`) and the
//!   state-service payload-category constant.
//! - `neo-data-cache` (Layer 1) - for the `DataCache` used during
//!   MPT computation.
//! - `neo-payloads` (Layer 1) - for the `ExtensiblePayload` used to
//!   transport state-root messages.
//! - `neo-event-handlers` (Layer 1) - for the typed
//!   `BlockCommittedHandler` / `BlockRevertedHandler` traits.
//!
//! Must **not** depend on `neo-core` (deleted).

#![doc(html_root_url = "https://docs.rs/neo-state-service/0.7.2")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod commit_handlers;
pub mod root_cache;
pub mod state_root;
pub mod state_store;
pub mod verification;

// Re-export the wire-protocol surface from `neo-state-types` so
// consumers can `use neo_state_service::*` to get the full state
// service surface (stateful + wire).
pub use neo_state_types::{
    Keys, MessageType, StateRootIngestStats, Vote, STATE_SERVICE_CATEGORY,
};

pub use root_cache::{
    StateRootCache, StateRootCacheEntry, StateRootCacheStats, StateRootCacheStatsSnapshot,
    DEFAULT_ROOT_CACHE_CAPACITY,
};
pub use state_root::{StateRoot, CURRENT_VERSION};
pub use state_store::{StateStore, StateStoreLookup, StateStoreTransaction};
