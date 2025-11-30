//! # Neo Storage
//!
//! Storage traits and abstractions for the Neo blockchain.
//!
//! This crate provides abstract interfaces for storage operations, allowing
//! different storage backends (RocksDB, memory, etc.) to be used interchangeably.
//!
//! ## Design Principles
//!
//! - **Trait-based abstraction**: Defines interfaces without concrete implementations
//! - **Breaks circular dependencies**: Allows smart_contract and ledger to depend on
//!   storage traits without depending on each other
//! - **Backend agnostic**: Works with any storage backend that implements the traits
//!
//! ## Core Traits
//!
//! - [`IReadOnlyStore`]: Read-only storage operations
//! - [`IWriteStore`]: Write operations (put, delete)
//! - [`IStore`]: Combined read/write operations
//! - [`ISnapshot`]: Point-in-time snapshot of storage
//!
//! ## Example
//!
//! ```rust,ignore
//! use neo_storage::{IReadOnlyStore, StorageKey, StorageItem};
//!
//! fn read_contract_state<S: IReadOnlyStore>(store: &S, key: &StorageKey) -> Option<StorageItem> {
//!     store.try_get(key)
//! }
//! ```

pub mod error;
pub mod key_builder;
pub mod traits;
pub mod types;

// Re-exports
pub use error::{StorageError, StorageResult};
pub use key_builder::{KeyBuilder, KeyBuilderError};
pub use traits::{IReadOnlyStore, IWriteStore, IStore, ISnapshot};
pub use types::{StorageKey, StorageItem, SeekDirection, TrackState};
