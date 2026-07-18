//! # neo-storage::persistence::data_cache
//!
//! Write-back cache implementation and tracked-entry state.
//!
//! ## Boundary
//!
//! This module belongs to `neo-storage`. This infrastructure crate owns store
//! mechanics and must not execute contracts, import blocks, or make RPC/network
//! policy decisions.
//!
//! ## Contents
//!
//! - `cache`: Cache state and mutation helpers.
//! - `storage_watch`: storage change watch records.
//! - `trackable`: tracked cache entry records.

/// Cache implementation and read-through helpers.
pub mod cache;
mod observer;
mod storage_watch;
/// Trackable storage entry state and cache configuration.
pub mod trackable;

pub use cache::{CacheRead, DataCache, EmptyCacheBacking};
pub use observer::{DataCacheReadObservationPause, DataCacheReadObserver, DataCacheReadOrigin};
pub use trackable::{DataCacheConfig, DataCacheError, DataCacheResult, Trackable, TrackableEntry};
