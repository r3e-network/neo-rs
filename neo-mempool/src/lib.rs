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

#![doc(html_root_url = "https://docs.rs/neo-mempool/0.9.0")]

mod admission;
mod events;
mod pool;

pub use admission::{
    PreverifyCompleted, TransactionRouter, TransactionVerificationContext, transaction_router,
    transaction_verification_context, verification, verify_state_dependent,
    verify_state_independent, verify_transaction,
};
pub use events::{
    NewTransactionEventArgs, TransactionRemovedEventArgs, new_transaction_event_args,
    transaction_removed_event_args,
};
pub use pool::{
    MemoryPool, NewTransactionCallback, PoolIndex, PoolItem, SharedMemoryPool,
    TransactionAddedCallback, TransactionRelayCallback, TransactionRemovedCallback, memory_pool,
    pool_index, pool_item,
};
