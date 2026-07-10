//! # neo-storage::persistence::providers
//!
//! Provider implementations behind the crate public traits.
//!
//! ## Boundary
//!
//! This module belongs to `neo-storage`. This infrastructure crate owns store
//! mechanics and must not execute contracts, import blocks, or make RPC/network
//! policy decisions.
//!
//! ## Contents
//!
//! - `memory_snapshot`: in-memory snapshot implementation.
//! - `memory_store`: in-memory store implementation.
//! - `memory_store_provider`: in-memory store provider.

/// Snapshot over an in-memory store.
pub mod memory_snapshot;
/// Ephemeral in-memory key/value store.
pub mod memory_store;
/// Provider that creates in-memory stores.
pub mod memory_store_provider;
/// Concrete enum for runtime-selected store backends.
pub mod runtime_store;

pub use memory_snapshot::MemorySnapshot;
pub use memory_store::MemoryStore;
pub use memory_store_provider::MemoryStoreProvider;
pub use runtime_store::RuntimeStore;
