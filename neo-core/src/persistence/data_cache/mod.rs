//! Shared in-memory cache for persistence providers.
//!
//! This module implements a Copy-on-Write (CoW) DataCache pattern for optimal
//! performance during block synchronization with optional LRU read caching
//! and intelligent prefetching for common access patterns.

pub mod cache;
pub mod trackable;
mod prefetch;
mod storage_watch;

pub use cache::{DataCache, OnEntryDelegate};
pub use prefetch::PrefetchPattern;
pub use trackable::{DataCacheConfig, DataCacheError, DataCacheResult, Trackable};

#[cfg(feature = "runtime")]
pub(crate) use storage_watch::{
    clear_storage_watch_context, set_storage_watch_context, StorageWatchPhase,
};

#[cfg(test)]
mod tests;
