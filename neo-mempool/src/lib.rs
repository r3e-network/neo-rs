//! neo-mempool: Memory pool for Neo blockchain transaction management.
//!
//! This crate provides the canonical mempool implementation extracted from
//! `neo-core::ledger::memory_pool`. It preserves all historical behaviors:
//! capacity, priority, fee, conflicts, oracle duplicate, removal reasons,
//! events, clash, snapshot admission, and quarantine lifecycle.
//!
//! # Dependency alignment
//!
//! - Consumes `neo_ledger::validation::TransactionVerificationContext` as the
//!   authoritative fee/oracle tracking context (task 86 authority).
//! - Consumes `neo_ledger::validation::TransactionVerificationContext::payer` as
//!   the public payer resolution API (task 86 approved typed authority).
//! - Consumes `neo_primitives::{VerifyResult, TransactionRemovalReason}` as
//!   single-source-of-truth protocol enums.
//! - Does NOT introduce ledger→mempool or mempool→core/system/native reverse
//!   dependencies.
//!
//! # Pending task 89 alignment
//!
//! - `TransactionVerificationContext::payer` must become `pub` (currently
//!   `pub(crate)` in neo-core). The `Payer` type alias
//!   `(UInt160, Option<UInt160>)` must also be publicly exported.
//! - Workspace root `Cargo.toml` registration (task 89 scope).
//! - Old source deletion from neo-core (task P9 scope, after 89 verification).

pub mod memory_pool;
pub mod new_transaction_event_args;
pub mod pool_item;
pub mod services;
pub mod transaction_removed_event_args;
pub mod transaction_router;

// Re-export primary types at crate root for ergonomic consumption.
pub use memory_pool::MemoryPool;
pub use memory_pool::{
    QuarantineConfig, QuarantineError, QuarantineExpiryReview, QuarantineMetadata,
};
pub use new_transaction_event_args::NewTransactionEventArgs;
pub use pool_item::PoolItem;
pub use services::{LockedMempoolService, MempoolService};
pub use transaction_removed_event_args::TransactionRemovedEventArgs;
pub use transaction_router::{PreverifyCompleted, TransactionRouter};

// Consume protocol enums from their single source of truth.
pub use neo_primitives::TransactionRemovalReason;
pub use neo_primitives::VerifyResult;

// Re-export TransactionVerificationContext from its authoritative location.
pub use neo_ledger::validation::TransactionVerificationContext;
