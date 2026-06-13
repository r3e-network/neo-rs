//! # neo-mempool
//!
//! Canonical home for the Neo transaction memory pool,
//! [`PoolItem`](pool_item), [`TransactionRouter`](transaction_router),
//! and the per-block [`TransactionVerificationContext`].
//!
//! ## Modules
//!
//! - [`memory_pool::MemoryPool`] — the two-queue (verified / unverified)
//!   priority mempool.
//! - [`pool_item::PoolItem`] — a `Transaction` wrapper holding
//!   mempool-side metadata.
//! - [`pool_index::PoolIndex`] — the BTreeMap-backed priority queue used
//!   by the mempool.
//! - [`transaction_verification_context::TransactionVerificationContext`]
//!   — the per-block set of confirmed-transaction hashes used to prune
//!   the pool on commit.
//! - [`transaction_router::TransactionRouter`] — the entry point that
//!   runs state-independent pre-verification before a transaction is
//!   admitted into the pool.
//! - [`new_transaction_event_args::NewTransactionEventArgs`] /
//!   [`transaction_removed_event_args::TransactionRemovedEventArgs`] —
//!   event payloads raised by the pool's subscriber callbacks.
//!
//! ## Layering
//!
//! Sits in **Layer 2 (service)**. Depends on:
//!
//! - `neo-payloads` (Layer 1) — for the `Transaction` data type.
//! - `neo-storage` (Layer 1) — for the `DataCache` used during
//!   state-dependent verification.
//! - `neo-execution` (Layer 1) — for `ApplicationEngine` used during
//!   witness verification (via the mempool's reverify path).
//! - `neo-config` (Layer 0) — for `ProtocolSettings`.
//!
//! Must **not** depend on `neo-core` (deleted), `neo-network`
//! (Layer 2), or any stateful runtime crate.

#![doc(html_root_url = "https://docs.rs/neo-mempool/0.7.2")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod memory_pool;
pub mod new_transaction_event_args;
pub mod pool_index;
pub mod pool_item;
pub mod transaction_removed_event_args;
pub mod transaction_router;
pub mod transaction_verification_context;
pub mod verification;

pub use memory_pool::{
    MemoryPool, NewTransactionCallback, SharedMemoryPool, TransactionAddedCallback,
    TransactionRelayCallback, TransactionRemovedCallback,
};
pub use new_transaction_event_args::NewTransactionEventArgs;
pub use pool_index::PoolIndex;
pub use pool_item::PoolItem;
pub use transaction_removed_event_args::TransactionRemovedEventArgs;
pub use transaction_router::{PreverifyCompleted, TransactionRouter};
pub use transaction_verification_context::TransactionVerificationContext;
pub use verification::{verify_state_dependent, verify_state_independent, verify_transaction};
