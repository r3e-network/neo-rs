//! Transaction router implementation.
//!
//! This module provides the TransactionRouter functionality exactly matching C# Neo TransactionRouter.

use crate::network::p2p::payloads::Transaction;
use crate::protocol_settings::ProtocolSettings;

mod preverify_completed;
pub use preverify_completed::PreverifyCompleted;

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
