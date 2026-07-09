//! # neo-mempool::admission
//!
//! Mempool admission, preverification, and transaction routing logic.
//!
//! ## Boundary
//!
//! This module belongs to `neo-mempool`. This service crate owns transaction
//! pool policy and must not persist blocks, run consensus, or expose RPC
//! transport details.
//!
//! ## Contents
//!
//! - `ledger_provider`: ledger read capabilities used by admission.
//! - `native_provider`: native contract read capabilities used by admission.
//! - `transaction_router`: mempool transaction router.
//! - `transaction_verification_context`: transaction verification context types
//!   and helpers.
//! - `verification`: validation verdicts and verification coverage.

mod ledger_provider;
mod native_provider;
pub mod transaction_router;
pub mod transaction_verification_context;
pub mod verification;

pub use transaction_router::{PreverifyCompleted, TransactionRouter};
pub use transaction_verification_context::TransactionVerificationContext;
pub use verification::{
    verify_state_dependent_with_native_provider, verify_state_independent,
    verify_transaction_dependent_only_with_native_provider,
    verify_transaction_with_native_provider,
};
