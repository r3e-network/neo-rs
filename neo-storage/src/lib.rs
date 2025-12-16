//! # Neo Storage
//!
//! Storage traits and types for the Neo blockchain.
//!
//! ## Crate Purpose
//!
//! This crate provides the **single source of truth** for all storage-related
//! functionality in the Neo ecosystem. It includes:
//!
//! - **Storage traits**: `IReadOnlyStore`, `IWriteStore`, `IStore`, `ISnapshot`
//! - **Storage types**: `StorageKey`, `StorageItem`, `SeekDirection`, `TrackState`
//! - **Cache**: `DataCache`, `Trackable` for in-memory caching with tracking
//! - **Hash utilities**: C#-compatible xxhash3 implementation for storage keys
//! - **Key building**: Fluent API for constructing storage keys
//!
//! ## Core Components
//!
//! - [`IReadOnlyStore`]: Read-only storage operations (try_get, contains)
//! - [`IWriteStore`]: Write operations (put, delete)
//! - [`IStore`]: Combined read/write interface
//! - [`ISnapshot`]: Point-in-time snapshot with seek/find operations
//! - [`StorageKey`]: Storage key with contract ID and key suffix (C# parity)
//! - [`StorageItem`]: Storage value with constant flag support
//! - [`DataCache`]: In-memory cache with change tracking
//! - [`SeekDirection`]: Forward/Backward iteration direction
//! - [`TrackState`]: Cache tracking states (None, Added, Changed, Deleted, NotFound)
//!
//! ## Example
//!
//! ```rust,ignore
//! use neo_storage::{IReadOnlyStore, StorageKey, StorageItem};
//! use neo_primitives::UInt160;
//!
//! fn read_value<S: IReadOnlyStore>(store: &S, key: &StorageKey) -> Option<StorageItem> {
//!     store.try_get(key)
//! }
//!
//! // Create storage key with UInt160
//! let hash = UInt160::zero();
//! let key = StorageKey::create_with_uint160(-1, 0x14, &hash);
//! ```

pub mod cache;
pub mod error;
pub mod hash_utils;
pub mod key_builder;
pub mod traits;
pub mod types;

// Re-exports
pub use cache::{ClonedCache, DataCache, DataCacheError, DataCacheResult, Trackable};
pub use error::{StorageError, StorageResult};
pub use hash_utils::{
    default_xx_hash3_seed, hash_code_combine_i32, xx_hash3_32, DEFAULT_XX_HASH3_SEED,
};
pub use key_builder::{KeyBuilder, KeyBuilderError};
pub use traits::{IReadOnlyStore, ISnapshot, IStore, IWriteStore};
pub use types::{SeekDirection, StorageItem, StorageKey, TrackState};
