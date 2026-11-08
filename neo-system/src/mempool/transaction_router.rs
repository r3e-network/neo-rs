//! Transaction router implementation.
//!
//! Distributed from `neo-core::ledger::transaction_router`. Provides the
//! `TransactionRouter` functionality matching C# Neo `TransactionRouter`.
//!
//! # Dependency adaptation
//!
//! The neo-core original used the `TransactionExt::verify_state_independent`
//! method. That extension trait lives in neo-core; the underlying logic is
//! exposed by `neo_ledger::validation::verify_transaction_state_independent`,
//! which this crate already consumes for its mempool validation path.

use neo_ledger::validation::verify_transaction_state_independent;
use neo_primitives::VerifyResult;
use neo_protocol::payloads::Transaction;
use neo_protocol::protocol_settings::ProtocolSettings;
use serde::{Deserialize, Serialize};

/// public record PreverifyCompleted(Transaction Transaction, bool Relay, VerifyResult Result);
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreverifyCompleted {
    /// The transaction that was verified.
    pub transaction: Transaction,
    /// Whether the transaction should be relayed to the network after verification.
    pub relay: bool,
    /// The outcome of the state-independent verification.
    pub result: VerifyResult,
}

/// Transaction router for handling transaction pre-verification
pub struct TransactionRouter {
    settings: ProtocolSettings,
}

impl TransactionRouter {
    /// Constructor from protocol settings
    pub fn new(settings: ProtocolSettings) -> Self {
        Self { settings }
    }

    /// Runs state-independent transaction verification before blockchain validation.
    pub fn preverify(&self, transaction: Transaction, relay: bool) -> PreverifyCompleted {
        let result = verify_transaction_state_independent(&transaction, &self.settings);

        PreverifyCompleted {
            transaction,
            relay,
            result,
        }
    }
}
