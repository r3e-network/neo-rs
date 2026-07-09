//! [`TransactionRouter`] - the entry point for transactions received
//! from the network.
//!
//! Performs the cheap, state-independent verification
//! (signature shape, size limits, fee bounds, etc.) before the
//! transaction is admitted into the [`crate::MemoryPool`] for
//! state-dependent (witness) verification.
//!
//! ## Cached Verification Result
//!
//! `PreverifyCompleted` carries the state-independent result as
//! `cached_state_independent`. This prevents double-verification
//! when the mempool admission path re-runs transaction verification:
//! if the cached result is available (and `Succeed`), the mempool
//! skips the redundant `verify_state_independent()` and only performs
//! the provider-aware state-dependent verifier. C# achieves the same via
//! `Transaction.VerificationResult` caching in
//! `Blockchain.AskForTransaction()`.

use neo_config::ProtocolSettings;
use neo_payloads::Transaction;
use neo_primitives::{Verifiable, VerifyResult};

use crate::verification::verify_state_independent;

/// Result of the state-independent pre-verification stage.
#[derive(Debug, Clone)]
pub struct PreverifyCompleted {
    /// The transaction that was pre-verified.
    pub transaction: Transaction,
    /// Whether the transaction was originally intended to be
    /// relayed (true) or merely accepted locally (false).
    pub relay: bool,
    /// The outcome of the lightweight structural verification
    /// (version, signers, witnesses). Maps to C#
    /// `VerificationResult`'s structural checks.
    pub result: VerifyResult,
    /// Cached state-independent verification outcome (signature
    /// shape, size, ECDSA fast-paths). `None` when state-independent
    /// verification was not performed; `Some(Succeed)` allows the
    /// downstream `MemoryPool::try_add_cached` to skip redundant
    /// signature re-verification. Mirrors C# `Transaction.
    /// VerificationResult` caching.
    pub cached_state_independent: Option<VerifyResult>,
}

impl PreverifyCompleted {
    /// Returns whether the pre-verification succeeded.
    pub fn is_success(&self) -> bool {
        self.result.is_success()
    }
}

/// Router for state-independent transaction pre-verification.
#[derive(Debug, Clone)]
pub struct TransactionRouter {
    settings: ProtocolSettings,
}

impl TransactionRouter {
    /// Constructs a new `TransactionRouter` from the supplied
    /// protocol settings.
    pub fn new(settings: ProtocolSettings) -> Self {
        Self { settings }
    }

    /// Returns the protocol settings this router was constructed with.
    pub fn settings(&self) -> &ProtocolSettings {
        &self.settings
    }

    /// Runs state-independent transaction verification.
    ///
    /// Mirrors C# `TransactionRouter` preverify: runs the lightweight
    /// structural check (`Verifiable::verify`) followed by the full
    /// state-independent verification (`verify_state_independent`).
    /// The cached result is carried in `PreverifyCompleted.
    /// cached_state_independent` so the downstream admission path
    /// (`MemoryPool::try_add_cached`) can skip redundant signature
    /// re-verification.
    pub fn preverify(&self, transaction: Transaction, relay: bool) -> PreverifyCompleted {
        let structural_ok = Verifiable::verify(&transaction);
        if !structural_ok {
            return PreverifyCompleted {
                transaction,
                relay,
                result: VerifyResult::Invalid,
                cached_state_independent: None,
            };
        }
        let state_independent = verify_state_independent(&transaction, &self.settings);
        let result = if state_independent == VerifyResult::Succeed {
            VerifyResult::Succeed
        } else {
            state_independent
        };
        PreverifyCompleted {
            transaction,
            relay,
            result,
            cached_state_independent: Some(state_independent),
        }
    }
}

#[cfg(test)]
#[path = "../tests/admission/transaction_router.rs"]
mod tests;
