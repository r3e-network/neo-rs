//! Typed transaction validation and admission outcomes.

use neo_payloads::Transaction;
use neo_primitives::{UInt256, VerifyResult};
use thiserror::Error;

use super::TransactionOrigin;

/// A transaction whose state-independent checks succeeded outside the pool
/// write lock.
#[derive(Debug)]
pub(crate) struct ValidatedTransaction {
    transaction: Transaction,
    origin: TransactionOrigin,
}

impl ValidatedTransaction {
    pub(super) const fn new(transaction: Transaction, origin: TransactionOrigin) -> Self {
        Self {
            transaction,
            origin,
        }
    }

    pub(crate) fn into_parts(self) -> (Transaction, TransactionOrigin) {
        (self.transaction, self.origin)
    }
}

/// Result of state-independent transaction validation.
#[derive(Debug)]
pub(crate) enum TransactionValidationOutcome {
    /// The transaction may proceed to atomic pool-context validation.
    Valid(ValidatedTransaction),
    /// The transaction is invalid independently of canonical or pool state.
    Rejected {
        /// Rejected transaction.
        transaction: Transaction,
        /// Submission origin.
        origin: TransactionOrigin,
        /// Canonical Neo verification verdict.
        result: VerifyResult,
    },
}

/// Infrastructure failure encountered before atomic pool mutation.
#[derive(Debug, Error)]
pub enum TransactionAdmissionError {
    /// The transaction hash could not be computed from its canonical encoding.
    #[error("failed to compute transaction hash: {0}")]
    InvalidHash(String),
    /// A required canonical provider read failed.
    #[error("transaction admission provider read `{operation}` failed: {message}")]
    ProviderRead {
        /// Failed provider operation.
        operation: &'static str,
        /// Provider error text.
        message: String,
    },
}

impl TransactionAdmissionError {
    pub(crate) fn provider(operation: &'static str, error: impl std::fmt::Display) -> Self {
        Self::ProviderRead {
            operation,
            message: error.to_string(),
        }
    }
}

/// Result of the single production transaction-admission operation.
#[derive(Debug)]
pub enum TransactionAdmissionOutcome {
    /// Transaction passed validation and remains in the verified pool.
    Accepted {
        /// Accepted transaction hash.
        hash: UInt256,
        /// Submission origin retained by the pool item.
        origin: TransactionOrigin,
    },
    /// Transaction was deterministically rejected by Neo admission policy.
    Rejected {
        /// Rejected transaction hash when canonical serialization succeeded.
        hash: Option<UInt256>,
        /// Submission origin.
        origin: TransactionOrigin,
        /// Canonical Neo verification verdict.
        result: VerifyResult,
    },
    /// Admission could not obtain required canonical state.
    Error {
        /// Hash when canonical hashing succeeded.
        hash: Option<UInt256>,
        /// Submission origin.
        origin: TransactionOrigin,
        /// Typed infrastructure failure.
        error: TransactionAdmissionError,
    },
}

impl TransactionAdmissionOutcome {
    /// Returns whether the transaction was accepted and retained.
    #[must_use]
    pub const fn is_accepted(&self) -> bool {
        matches!(self, Self::Accepted { .. })
    }

    /// Returns the transaction hash when available.
    #[must_use]
    pub const fn hash(&self) -> Option<UInt256> {
        match self {
            Self::Accepted { hash, .. } => Some(*hash),
            Self::Rejected { hash, .. } | Self::Error { hash, .. } => *hash,
        }
    }

    /// Returns the canonical verdict exposed by existing Neo wire/RPC surfaces.
    #[must_use]
    pub const fn verify_result(&self) -> VerifyResult {
        match self {
            Self::Accepted { .. } => VerifyResult::Succeed,
            Self::Rejected { result, .. } => *result,
            Self::Error { .. } => VerifyResult::UnableToVerify,
        }
    }

    /// Returns the submission origin.
    #[must_use]
    pub const fn origin(&self) -> TransactionOrigin {
        match self {
            Self::Accepted { origin, .. }
            | Self::Rejected { origin, .. }
            | Self::Error { origin, .. } => *origin,
        }
    }
}
