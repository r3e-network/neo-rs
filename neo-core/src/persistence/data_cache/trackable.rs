use crate::persistence::read_cache::ReadCacheConfig;
use crate::smart_contract::{StorageItem, StorageKey};
use std::collections::{HashMap, HashSet};
use thiserror::Error;

/// Represents an entry in the cache.
pub type Trackable = neo_storage::cache::TrackableEntry<StorageItem>;

/// Internal state protected by RwLock for thread-safe Copy-on-Write
pub(crate) struct InnerState {
    pub(crate) dictionary: HashMap<StorageKey, Trackable>,
    pub(crate) change_set: HashSet<StorageKey>,
}

impl InnerState {
    pub(crate) fn new() -> Self {
        Self {
            dictionary: HashMap::new(),
            change_set: HashSet::new(),
        }
    }
}

/// Configuration for DataCache.
#[derive(Debug, Clone, Copy)]
pub struct DataCacheConfig {
    /// Maximum number of entries in the write cache
    pub max_entries: usize,
    /// Whether reads should be mirrored into the write cache dictionary.
    pub track_reads_in_write_cache: bool,
    /// Enable read caching with LRU
    pub enable_read_cache: bool,
    /// Read cache configuration
    pub read_cache_config: ReadCacheConfig,
    /// Enable intelligent prefetching based on access patterns
    pub enable_prefetching: bool,
    /// Number of items to prefetch when pattern detected
    pub prefetch_count: usize,
    /// Minimum confidence threshold for prefetching (0-100)
    pub prefetch_confidence_threshold: u8,
}

impl Default for DataCacheConfig {
    fn default() -> Self {
        Self {
            max_entries: 100000,
            track_reads_in_write_cache: true,
            enable_read_cache: true,
            read_cache_config: ReadCacheConfig::default(),
            enable_prefetching: true,
            prefetch_count: 10,
            prefetch_confidence_threshold: 30,
        }
    }
}

/// Errors returned by DataCache operations.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DataCacheError {
    #[error("cache is read-only")]
    ReadOnly,
    #[error("unable to commit changes: {0}")]
    CommitFailed(String),
}

pub type DataCacheResult<T = ()> = Result<T, DataCacheError>;
