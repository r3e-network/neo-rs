//! # neo-serialization::providers
//!
//! Provider implementations behind the crate public traits.
//!
//! ## Boundary
//!
//! This module belongs to `neo-serialization`. This codec crate owns
//! serialization adapters and must not run services, import blocks, or mutate
//! ledger state.
//!
//! ## Contents
//!
//! - `providers`: serialization provider adapters and store re-exports.

pub use neo_storage::persistence::providers::memory_snapshot;
pub use neo_storage::persistence::providers::memory_store;
pub use neo_storage::persistence::providers::memory_store_provider;

pub use memory_snapshot::MemorySnapshot;
pub use memory_store::MemoryStore;
pub use memory_store_provider::MemoryStoreProvider;

// Re-export the RocksDB backend here (under the same `providers::rocksdb` path)
// so existing `neo_core::persistence::providers::{RocksDBStoreProvider,
// rocksdb::*}` callers are unaffected.
pub use neo_storage::rocksdb;

pub use neo_storage::rocksdb::RocksDBStoreProvider;
