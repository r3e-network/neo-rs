//! Transaction router implementation.
//!
//! This module provides the TransactionRouter functionality exactly matching C# Neo TransactionRouter.

use super::VerifyResult;
use crate::network::p2p::payloads::Transaction;
use crate::protocol_settings::ProtocolSettings;
use serde::{Deserialize, Serialize};

/// namespace Neo.Ledger -> internal class TransactionRouter(NeoSystem system) : UntypedActor
/// public record Preverify(Transaction Transaction, bool Relay);
#[derive(Debug, Clone)]
pub struct Preverify {
    pub transaction: Transaction,
    pub relay: bool,
}

/// public record PreverifyCompleted(Transaction Transaction, bool Relay, VerifyResult Result);
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreverifyCompleted {
    pub transaction: Transaction,
    pub relay: bool,
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

    /// protected override void OnReceive(object message)
    pub fn on_receive(&self, message: &Preverify) -> PreverifyCompleted {
        // var send = new PreverifyCompleted(preverify.Transaction, preverify.Relay,
        //         preverify.Transaction.VerifyStateIndependent(_system.Settings));
        let result = message.transaction.verify_state_independent(&self.settings);

        PreverifyCompleted {
            transaction: message.transaction.clone(),
            relay: message.relay,
            result,
        }
    }
}
