//! # neo-mempool
//!
//! Transaction memory-pool admission, indexing, events, and routing.
//!
//! ## Boundary
//!
//! This service crate owns transaction pool policy and must not persist blocks,
//! run consensus, or expose RPC transport details.
//!
//! ## Contents
//!
//! - `admission`: Mempool admission, preverification, and transaction routing
//!   logic.
//! - `events`: Mempool event records emitted to subscribers.
//! - `pool`: Memory-pool indexes, items, and mutation helpers.

#![doc(html_root_url = "https://docs.rs/neo-mempool/0.10.0")]

mod admission;
mod pool;

pub use admission::{
    PreverifyCompleted, TransactionRouter, TransactionVerificationContext, transaction_router,
    transaction_verification_context, verification, verify_state_dependent_with_native_provider,
    transaction_witnesses_are_state_independent, verify_state_independent,
    verify_transaction_dependent_only_with_native_provider,
    verify_transaction_with_native_provider,
};
pub use pool::{
    MemoryPool, PoolIndex, PoolItem, SharedMemoryPool, memory_pool, pool_index, pool_item,
};
