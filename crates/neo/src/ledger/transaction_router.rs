//! Transaction router implementation.
//!
//! This module provides the TransactionRouter functionality exactly matching C# Neo TransactionRouter.

// Matches C# using directives exactly:
// using Akka.Actor;
// using Akka.Routing;
// using Neo.Network.P2P.Payloads;
// using System;

use super::VerifyResult;
use crate::network::p2p::payloads::Transaction;
use crate::protocol_settings::ProtocolSettings;

/// namespace Neo.Ledger -> internal class TransactionRouter(NeoSystem system) : UntypedActor

/// public record Preverify(Transaction Transaction, bool Relay);
pub struct Preverify {
    pub transaction: Transaction,
    pub relay: bool,
}

/// public record PreverifyCompleted(Transaction Transaction, bool Relay, VerifyResult Result);
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
    /// Constructor (primary constructor in C#)
    pub(crate) fn new(system: &crate::system::NeoSystem) -> Self {
        Self {
            settings: system.settings().clone(),
        }
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

        // _system.Blockchain.Tell(send, Sender);
        // Note: In Rust, the actor system communication would be handled differently
    }

    // internal static Props Props(NeoSystem system)
    // {
    //     return Akka.Actor.Props.Create(() => new TransactionRouter(system)).WithRouter(new SmallestMailboxPool(Environment.ProcessorCount));
    // }
    // Note: Actor Props would be handled differently in Rust actor systems
}
