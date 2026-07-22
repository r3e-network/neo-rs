//! # neo-mempool::admission
//!
//! Mempool admission and transaction verification logic.
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
//! - `origin`: typed transaction source and propagation policy.
//! - `outcome`: typed validation and admission outcomes.
//! - `transaction_verification_context`: transaction verification context types
//!   and helpers.
//! - `validator`: state-independent validation before pool locking.
//! - `verification`: validation verdicts and verification coverage.

mod ledger_provider;
mod native_provider;
mod origin;
mod outcome;
pub mod transaction_verification_context;
mod validator;
pub mod verification;

pub(crate) use validator::validate_state_independent;

pub use ledger_provider::AdmissionLedgerProvider;
#[cfg(test)]
pub(crate) use ledger_provider::NativeAdmissionLedgerProvider;
pub use origin::TransactionOrigin;
pub use outcome::{TransactionAdmissionError, TransactionAdmissionOutcome};
pub(crate) use outcome::{TransactionValidationOutcome, ValidatedTransaction};
pub use transaction_verification_context::TransactionVerificationContext;
pub use verification::{
    verify_state_dependent_with_native_provider, verify_state_independent,
    verify_transaction_with_native_provider,
};
