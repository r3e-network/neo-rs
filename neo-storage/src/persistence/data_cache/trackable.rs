use crate::persistence::data_cache::prefetch::PrefetchConfig;
use crate::persistence::read_cache::ReadCacheConfig;
use crate::types::{StorageItem, StorageKey};
use std::collections::HashMap;
use thiserror::Error;

/// Represents an entry in the cache.
pub type Trackable = crate::cache::TrackableEntry<StorageItem>;

/// Internal state protected by RwLock for thread-safe Copy-on-Write
pub(crate) struct InnerState {
    pub(crate) dictionary: HashMap<StorageKey, Trackable>,
}

impl InnerState {
    pub(crate) fn new() -> Self {
        Self {
            dictionary: HashMap::new(),
        }
    }
}

/// Configuration for PersistenceDataCache.
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
    /// Enable the sharded hot-key cache for committed storage reads
    pub enable_hot_key_cache: bool,
    /// Total entry bound of the hot-key cache (spread over a fixed shard count)
    pub hot_key_cache_capacity: usize,
    /// Speculative prefetch configuration (disabled unless explicitly enabled)
    pub prefetch: PrefetchConfig,
}

impl Default for DataCacheConfig {
    fn default() -> Self {
        Self {
            max_entries: 100000,
            track_reads_in_write_cache: true,
            enable_read_cache: true,
            read_cache_config: ReadCacheConfig::default(),
            enable_hot_key_cache: true,
            hot_key_cache_capacity:
                crate::persistence::hot_key_cache::DEFAULT_HOT_KEY_CACHE_CAPACITY,
            prefetch: PrefetchConfig::default(),
        }
    }
}

/// Errors returned by PersistenceDataCache operations.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DataCacheError {
    #[error("cache is read-only")]
    ReadOnly,
    #[error("invalid cache state transition: {0}")]
    InvalidState(String),
    #[error("unable to commit changes: {0}")]
    CommitFailed(String),
}

pub type DataCacheResult<T = ()> = Result<T, DataCacheError>;
