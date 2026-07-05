//! # neo-state-service::storage
//!
//! MPT storage, state-root cache, durable state records, and immutable
//! provider views.
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
//! - `state_store`: state-service store facade.

pub mod mpt_store;
pub mod root_cache;
pub mod state_store;

pub use mpt_store::{MptChange, MptReadSnapshot, MptStore};
pub use root_cache::{
    DEFAULT_ROOT_CACHE_CAPACITY, StateRootCache, StateRootCacheEntry, StateRootCacheStats,
    StateRootCacheStatsSnapshot,
};
pub use state_store::{StateStore, StateStoreLookup, StateStoreTransaction};
