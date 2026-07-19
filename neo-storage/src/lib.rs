//! # neo-storage
//!
//! Store traits, cache overlays, isolated maintenance metadata,
//! storage-domain types, and concrete backends.
//!
//! ## Boundary
//!
//! This infrastructure crate owns store mechanics and must not execute
//! contracts, import blocks, or make RPC/network policy decisions.
//!
//! ## Contents
//!
//! - `core`: Core reader, writer, var-int, and macro helpers for binary IO.
//! - `errors`: Typed errors and result aliases for this crate boundary.
//! - `persistence`: Persistence traits, snapshots, transactions, maintenance
//!   batches, and cache overlays.
//! - `mdbx`: Production default MDBX provider and store adapter.
//! - `types`: Storage-domain types shared by store implementations.

mod core;
mod errors;
pub mod mdbx;
/// Persistence traits, caches, snapshots, and in-memory store providers.
pub mod persistence;
pub mod types;

// Canonical cache types live in `persistence::data_cache`; re-export the common
// surface at the crate root for ergonomic access.
pub use core::{DEFAULT_XX_HASH3_SEED, KeyBuilder, KeyBuilderError, XxHash3};
pub use core::{hash_utils, key_builder};
pub use errors::{StorageError, StorageResult, error};
pub use persistence::data_cache::{
    CacheRead, DataCache, DataCacheAtomicMergeError, DataCacheError, DataCacheReadObservationPause,
    DataCacheReadObserver, DataCacheReadOrigin, DataCacheResult, DataCacheVersion,
    EmptyCacheBacking, LockedDataCacheView, Trackable, TrackableEntry,
};
pub use types::{SeekDirection, StorageItem, StorageItemCache, StorageKey, TrackState};
