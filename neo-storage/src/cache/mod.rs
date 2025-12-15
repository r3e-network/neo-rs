//! Cache implementations for Neo storage.
//!
//! This module provides caching layers for blockchain storage:
//! - [`DataCache`]: Core in-memory cache with tracking support
//! - [`Trackable`]: Entry wrapper with state tracking
//! - [`ClonedCache`]: Lightweight copy-on-write cache wrapper
//!
//! # Design
//!
//! The cache system is designed to:
//! 1. Minimize database reads through in-memory caching
//! 2. Track changes for efficient batch commits
//! 3. Support snapshot isolation for transaction verification
//!
//! # Example
//!
//! ```rust,ignore
//! use neo_storage::cache::{DataCache, Trackable};
//! use neo_storage::types::{StorageKey, StorageItem, TrackState};
//!
//! let cache = DataCache::new(false); // writable
//! let key = StorageKey::new(-1, vec![0x01]);
//! let value = StorageItem::new(vec![0xAA]);
//!
//! cache.add(key.clone(), value);
//! assert!(cache.contains(&key));
//! ```

mod cloned_cache;
mod data_cache;
mod trackable;

pub use cloned_cache::ClonedCache;
pub use data_cache::{DataCache, DataCacheError, DataCacheResult};
pub use trackable::Trackable;
