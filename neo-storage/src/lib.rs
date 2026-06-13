
//! # Neo Storage
//!
//! Storage traits and types for the Neo blockchain.
//!
//! ## Crate Purpose
//!
//! This crate provides the **single source of truth** for all storage-related
//! functionality in the Neo ecosystem. It includes:
//!
//! - **Storage traits**: `ReadOnlyStore`, `WriteStore`, `Store`, `StoreSnapshot`
//! - **Storage types**: `StorageKey`, `StorageItem`, `SeekDirection`, `TrackState`
//! - **Cache**: `DataCache`, `Trackable` for in-memory caching with tracking
//! - **Hash utilities**: C#-compatible xxhash3 implementation for storage keys
//! - **Key building**: Fluent API for constructing storage keys
//!
//! ## Core Components
//!
//! - [`ReadOnlyStore`]: Read-only storage operations (`try_get`, contains)
//! - [`WriteStore`]: Write operations (put, delete)
//! - [`Store`]: Combined read/write interface
//! - [`StoreSnapshot`]: Point-in-time snapshot with seek/find operations
//! - [`StorageKey`]: Storage key with contract ID and key suffix (C# parity)
//! - [`StorageItem`]: Storage value with constant flag support
//! - [`DataCache`]: In-memory cache with change tracking
//! - [`SeekDirection`]: Forward/Backward iteration direction
//! - [`TrackState`]: Cache tracking states (None, Added, Changed, Deleted, `NotFound`)
//!
//! ## Example
//!
//! ```rust,ignore
//! use neo_storage::{ReadOnlyStore, StorageKey, StorageItem};
//! use neo_primitives::UInt160;
//!
//! fn read_value<S: ReadOnlyStore>(store: &S, key: &StorageKey) -> Option<StorageItem> {
//!     store.try_get(key)
//! }
//!
//! // Create storage key with UInt160
//! let hash = UInt160::zero();
//! let key = StorageKey::create_with_uint160(-1, 0x14, &hash);
//! ```

pub mod error;
pub mod hash_utils;
pub mod key_builder;
pub mod persistence;
#[cfg(feature = "rocksdb")]
pub mod rocksdb;
pub mod types;

// Canonical cache types live in `persistence::data_cache`; re-export the common
// surface at the crate root for ergonomic access.
pub use error::{StorageError, StorageResult};
pub use hash_utils::{
    DEFAULT_XX_HASH3_SEED, default_xx_hash3_seed, hash_code_combine_i32, xx_hash3_32,
};
pub use key_builder::{KeyBuilder, KeyBuilderError};
pub use persistence::data_cache::{
    DataCache, DataCacheError, DataCacheResult, Trackable, TrackableEntry,
};
pub use types::{SeekDirection, StorageItem, StorageKey, TrackState};
