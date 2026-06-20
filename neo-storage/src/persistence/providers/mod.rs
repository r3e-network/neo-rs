//! In-memory persistence provider implementations.

/// Snapshot over an in-memory store.
pub mod memory_snapshot;
/// Ephemeral in-memory key/value store.
pub mod memory_store;
/// Provider that creates in-memory stores.
pub mod memory_store_provider;

pub use memory_snapshot::MemorySnapshot;
pub use memory_store::MemoryStore;
pub use memory_store_provider::MemoryStoreProvider;
