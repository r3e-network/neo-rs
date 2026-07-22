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
//! - `config`: immutable operator policy for the memory pool.
//! - `memory_pool`: memory-pool state and mutation API.
//! - `pool_index`: memory-pool priority indexes.
//! - `pool_item`: memory-pool item records.
//! - `state`: private queue/context state used by `memory_pool`.

mod config;
pub mod memory_pool;
mod pool_index;
pub mod pool_item;
mod state;

pub use config::{DEFAULT_MAX_TRANSACTIONS, TxPoolConfig, TxPoolConfigError};
pub use memory_pool::MemoryPool;
pub use pool_item::PoolItem;
