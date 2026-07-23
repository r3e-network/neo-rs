//! # neo-state-service
//!
//! State-root service, MPT persistence, and state-root protocol types.
//!
//! ## Boundary
//!
//! This service crate owns state-root and MPT service behavior and must not own
//! block download, consensus, RPC transport, or UI composition.
//!
//! ## Contents
//!
//! - `providers`: Frozen, statically dispatched state views and factories.
//! - `protocol`: Protocol enums, versioned records, and chain-level domain
//!   constants.
//! - `service`: Service loops, handles, lifecycle helpers, and command
//!   processing.
//! - `storage`: MPT storage, state-root cache, and durable state records.

#![doc(html_root_url = "https://docs.rs/neo-state-service/0.11.1")]

mod protocol;
pub mod providers;
mod service;
mod storage;

/// Extensible payload category for state service messages
/// (matches C# `StateService.StatePayloadCategory`).
pub const STATE_SERVICE_CATEGORY: &str = "StateService";

pub use protocol::{
    CURRENT_VERSION, Keys, MessageType, StateRoot, StateRootApplyMetrics, StateRootApplyStats,
    StateRootIngestMetrics, StateRootIngestStats, Vote, keys, message_type, metrics, state_root,
    vote,
};

pub use providers::{
    MptStateProvider, MptStateProviderFactory, StateEntry, StateProof, StateProviderError,
    StateProviderFactory, StateProviderResult, StateView, verify_state_proof,
};

pub use service::commit_handlers;
pub use storage::mpt_store::{MPT_NODE_KEY_BYTES, MPT_NODE_PREFIX, is_mpt_node_key};
pub use storage::{
    DEFAULT_ROOT_CACHE_CAPACITY, StateRootCache, StateRootCacheEntry, StateRootCacheStats,
    StateRootCacheStatsSnapshot,
};
pub use storage::{
    MDBX_STATE_SERVICE_NAMESPACE, MptChange, MptNodeReadGeneration, MptNodeReadSnapshot,
    MptNodeSnapshotFactory, MptReadSnapshot, MptStore, StateRootRecordError, StateStore,
    StateStoreLookup, StateStoreTransaction, decode_current_local_root_index,
    decode_local_state_root_record, decode_state_root_record, mpt_store, read_current_local_root,
    read_current_local_root_from, read_local_state_root, root_cache, state_store,
};
