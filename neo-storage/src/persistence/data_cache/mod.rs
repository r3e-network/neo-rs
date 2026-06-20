//! Neo storage cache primitives.

/// Cache implementation and read-through helpers.
pub mod cache;
mod storage_watch;
/// Trackable storage entry state and cache configuration.
pub mod trackable;

pub use cache::{DataCache, OnEntryDelegate};
pub use trackable::{DataCacheConfig, DataCacheError, DataCacheResult, Trackable, TrackableEntry};
