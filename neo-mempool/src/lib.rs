//! # neo-mempool
//!
//! Transaction memory-pool admission, indexing, events, and policy.
//!
//! ## Boundary
//!
//! This service crate owns transaction pool policy and must not persist blocks,
//! run consensus, or expose RPC transport details.
//!
//! ## Contents
//!
//! - `admission`: Typed mempool admission and transaction verification.
//! - `pool`: Runtime policy, memory-pool indexes, items, and mutation helpers.

#![doc(html_root_url = "https://docs.rs/neo-mempool/0.11.1")]

mod admission;
mod pool;

pub use admission::{
    AdmissionLedgerProvider, TransactionAdmissionError, TransactionAdmissionOutcome,
    TransactionOrigin, TransactionVerificationContext, transaction_verification_context,
    verification, verify_state_dependent_with_native_provider, verify_state_independent,
    verify_transaction_with_native_provider,
};
pub use pool::{
    DEFAULT_MAX_TRANSACTIONS, MemoryPool, PoolItem, TxPoolConfig, TxPoolConfigError, memory_pool,
    pool_item,
};
