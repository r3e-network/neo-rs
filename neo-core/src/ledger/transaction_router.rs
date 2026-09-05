//! Transaction router implementation.
//!
//! This module provides the TransactionRouter functionality exactly matching C# Neo TransactionRouter.

use super::VerifyResult;
use crate::network::p2p::payloads::Transaction;
use crate::protocol_settings::ProtocolSettings;
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
        let result = transaction.verify_state_independent(&self.settings);

        PreverifyCompleted {
            transaction,
            relay,
            result,
        }
    }
}
