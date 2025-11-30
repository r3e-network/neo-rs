//! Internal actor implementations for the neo_system module.
//!
//! This module contains actor implementations that handle asynchronous
//! message processing within the Neo system. Actors follow the Akka-style
//! actor model for concurrent, fault-tolerant processing.
//!
//! # Actors
//!
//! - `TransactionRouterActor` - Routes and pre-verifies incoming transactions

use std::any::Any;
use std::sync::Arc;

use akka::{Actor, ActorContext, ActorRef, ActorResult, Props};
use async_trait::async_trait;
use tracing::warn;

use crate::ledger::blockchain::{BlockchainCommand, PreverifyCompleted};
use crate::network::p2p::payloads::transaction::Transaction;
use crate::protocol_settings::ProtocolSettings;

/// Actor responsible for routing transaction verification requests.
///
/// This actor performs state-independent transaction verification and
/// forwards the results to the blockchain actor.
pub(crate) struct TransactionRouterActor {
    settings: Arc<ProtocolSettings>,
    blockchain: ActorRef,
}

impl TransactionRouterActor {
    /// Creates a new transaction router actor.
    fn new(settings: Arc<ProtocolSettings>, blockchain: ActorRef) -> Self {
        Self {
            settings,
            blockchain,
        }
    }

    /// Creates Props for spawning a transaction router actor.
    pub(crate) fn props(settings: Arc<ProtocolSettings>, blockchain: ActorRef) -> Props {
        Props::new(move || Self::new(settings.clone(), blockchain.clone()))
    }
}

#[async_trait]
impl Actor for TransactionRouterActor {
    async fn handle(
        &mut self,
        envelope: Box<dyn Any + Send>,
        ctx: &mut ActorContext,
    ) -> ActorResult {
        match envelope.downcast::<TransactionRouterMessage>() {
            Ok(message) => {
                match *message {
                    TransactionRouterMessage::Preverify { transaction, relay } => {
                        let result = transaction.verify_state_independent(&self.settings);
                        let completed = PreverifyCompleted {
                            transaction,
                            relay,
                            result,
                        };
                        if let Err(error) = self.blockchain.tell_from(
                            BlockchainCommand::PreverifyCompleted(completed),
                            ctx.sender(),
                        ) {
                            warn!(
                                target: "neo",
                                %error,
                                "failed to deliver preverify result to blockchain actor"
                            );
                        }
                    }
                }
                Ok(())
            }
            Err(payload) => {
                warn!(
                    target: "neo",
                    message_type = ?(*payload).type_id(),
                    "unknown message routed to transaction router actor"
                );
                Ok(())
            }
        }
    }
}

/// Messages handled by the transaction router actor.
#[derive(Debug)]
pub enum TransactionRouterMessage {
    /// Request to pre-verify a transaction before full validation.
    Preverify {
        /// The transaction to verify.
        transaction: Transaction,
        /// Whether to relay the transaction after verification.
        relay: bool,
    },
}
