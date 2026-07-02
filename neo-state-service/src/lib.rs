//! # neo-state-service
//!
//! State-root service, MPT persistence, validation, and state-root protocol
//! types.
//!
//! ## Boundary
//!
//! This service crate owns state-root and MPT service behavior and must not own
//! block download, consensus, RPC transport, or UI composition.
//!
//! ## Contents
//!
//! - `protocol`: Protocol enums, versioned records, and chain-level domain
//!   constants.
//! - `service`: Service loops, handles, lifecycle helpers, and command
//!   processing.
//! - `storage`: MPT storage, state-root cache, durable state records, and
//!   immutable provider views.
//! - `validation`: Validation routines and typed verdicts for protocol data.

#![doc(html_root_url = "https://docs.rs/neo-state-service/0.9.0")]

mod protocol;
mod service;
mod storage;
mod validation;

/// Extensible payload category for state service messages
/// (matches C# `StateService.StatePayloadCategory`).
pub const STATE_SERVICE_CATEGORY: &str = "StateService";

pub use protocol::{
    CURRENT_VERSION, Keys, MessageType, StateRoot, StateRootApplyMetrics, StateRootApplyStats,
    StateRootIngestMetrics, StateRootIngestStats, Vote, keys, message_type, metrics, state_root,
    vote,
};

pub use service::commit_handlers;
pub use storage::{
    DEFAULT_ROOT_CACHE_CAPACITY, StateRootCache, StateRootCacheEntry, StateRootCacheStats,
    StateRootCacheStatsSnapshot,
};
pub use storage::{
    MptChange, MptReadSnapshot, MptStore, StateStore, StateStoreLookup, StateStoreTransaction,
    mpt_store, root_cache, state_provider, state_store,
};
pub use storage::{MptStateProviderFactory, MptStateView, StateProviderFactory, StateView};
pub use validation::{StateRootCalculator, Verifier, VerifyOutcome, verification};
