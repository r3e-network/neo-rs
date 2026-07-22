//! # neo-state-service::storage
//!
//! MPT storage, state-root cache, and durable state records.
//!
//! ## Boundary
//!
//! This module belongs to `neo-state-service`. This service crate owns state-
//! root and MPT service behavior and must not own block download, consensus,
//! RPC transport, or UI composition.
//!
//! ## Contents
//!
//! - `mpt_store`: MPT-backed state store.
//! - `root_cache`: state-root cache.
//! - `state_root_records`: strict persisted state-root record codecs.
//! - `state_store`: state-service store facade.

pub mod mpt_store;
pub mod root_cache;
pub mod state_root_records;
pub mod state_store;

pub use mpt_store::{
    MDBX_STATE_SERVICE_NAMESPACE, MptChange, MptNodeReadGeneration, MptNodeReadSnapshot,
    MptNodeSnapshotFactory, MptReadSnapshot, MptStore,
};
pub use root_cache::{
    DEFAULT_ROOT_CACHE_CAPACITY, StateRootCache, StateRootCacheEntry, StateRootCacheStats,
    StateRootCacheStatsSnapshot,
};
pub use state_root_records::{
    StateRootRecordError, decode_current_local_root_index, decode_local_state_root_record,
    decode_state_root_record, read_current_local_root, read_current_local_root_from,
    read_local_state_root,
};
pub use state_store::{StateStore, StateStoreLookup, StateStoreTransaction};
