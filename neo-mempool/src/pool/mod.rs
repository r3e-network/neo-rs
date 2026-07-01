//! # neo-mempool::pool
//!
//! Memory-pool indexes, items, and mutation helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-mempool`. This service crate owns transaction
//! pool policy and must not persist blocks, run consensus, or expose RPC
//! transport details.
//!
//! ## Contents
//!
//! - `memory_pool`: memory-pool state and mutation API.
//! - `pool_index`: memory-pool priority indexes.
//! - `pool_item`: memory-pool item records.

pub mod memory_pool;
pub mod pool_index;
pub mod pool_item;

pub use memory_pool::{
    MemoryPool, NewTransactionCallback, SharedMemoryPool, TransactionAddedCallback,
    TransactionRelayCallback, TransactionRemovedCallback,
};
pub use pool_index::PoolIndex;
pub use pool_item::PoolItem;
