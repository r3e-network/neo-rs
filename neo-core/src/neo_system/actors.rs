//! Internal task handles for the neo_system module.
//!
//! This module contains lightweight typed task handles that process internal
//! Neo system work without routing everything through boxed actor messages.
//!
//! # Tasks
//!
//! - `TransactionRouterHandle` - Routes and pre-verifies incoming transactions

use std::sync::Arc;

use crate::ledger::transaction_router::{Preverify, TransactionRouter};
use crate::runtime::{ActorRef, AkkaError, AkkaResult};
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::warn;

use crate::ledger::blockchain::BlockchainCommand;
use crate::network::p2p::payloads::transaction::Transaction;
use crate::protocol_settings::ProtocolSettings;

const TX_ROUTER_MAILBOX_CAPACITY: usize = 65_536;

struct TransactionRouterEnvelope {
    message: TransactionRouterMessage,
    sender: Option<ActorRef>,
}

/// Typed handle responsible for routing transaction verification requests.
///
/// It performs state-independent transaction verification and forwards the
/// results to the blockchain actor without the custom boxed actor facade.
#[derive(Clone, Debug)]
pub struct TransactionRouterHandle {
    sender: mpsc::Sender<TransactionRouterEnvelope>,
    task: Arc<JoinHandle<()>>,
}

impl TransactionRouterHandle {
    /// Spawns a typed transaction-router worker.
    pub(crate) fn spawn(settings: Arc<ProtocolSettings>, blockchain: ActorRef) -> Self {
        let (sender, receiver) = mpsc::channel(TX_ROUTER_MAILBOX_CAPACITY);
        let router = TransactionRouter::new((*settings).clone());
        let task = Arc::new(tokio::spawn(run_transaction_router(
            router, blockchain, receiver,
        )));

        Self { sender, task }
    }

    /// Stops the router worker task.
    pub fn abort(&self) {
        self.task.abort();
    }

    /// Sends a message without specifying a sender.
    pub fn tell(&self, message: TransactionRouterMessage) -> AkkaResult<()> {
        self.tell_from(message, None)
    }

    /// Sends a message without specifying a sender, awaiting mailbox capacity.
    pub async fn tell_async(&self, message: TransactionRouterMessage) -> AkkaResult<()> {
        self.tell_from_async(message, None).await
    }

    /// Sends a message with an optional actor sender for relay responses.
    pub fn tell_from(
        &self,
        message: TransactionRouterMessage,
        sender: Option<ActorRef>,
    ) -> AkkaResult<()> {
        self.sender
            .try_send(TransactionRouterEnvelope { message, sender })
            .map_err(|error| AkkaError::send(error.to_string()))
    }

    /// Sends a message with backpressure and an optional actor sender.
    pub async fn tell_from_async(
        &self,
        message: TransactionRouterMessage,
        sender: Option<ActorRef>,
    ) -> AkkaResult<()> {
        self.sender
            .send(TransactionRouterEnvelope { message, sender })
            .await
            .map_err(|error| AkkaError::send(error.to_string()))
    }
}

async fn run_transaction_router(
    router: TransactionRouter,
    blockchain: ActorRef,
    mut receiver: mpsc::Receiver<TransactionRouterEnvelope>,
) {
    while let Some(envelope) = receiver.recv().await {
        match envelope.message {
            TransactionRouterMessage::Preverify { transaction, relay } => {
                let completed = router.on_receive(&Preverify { transaction, relay });
                if let Err(error) = blockchain.tell_from(
                    BlockchainCommand::PreverifyCompleted(completed),
                    envelope.sender,
                ) {
                    warn!(
                        target: "neo",
                        %error,
                        "failed to deliver preverify result to blockchain actor"
                    );
                }
            }
        }
    }
}

/// Messages handled by the transaction router worker.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::blockchain::PreverifyCompleted;
    use crate::ledger::VerifyResult;
    use crate::runtime::{Actor, ActorContext, ActorResult, ActorSystem, Props};
    use async_trait::async_trait;
    use parking_lot::Mutex;
    use std::any::Any;
    use std::time::Duration;
    use tokio::sync::oneshot;
    use tokio::time::timeout;

    struct BlockchainProbe {
        completed: Arc<Mutex<Option<oneshot::Sender<PreverifyCompleted>>>>,
    }

    #[async_trait]
    impl Actor for BlockchainProbe {
        async fn handle(
            &mut self,
            envelope: Box<dyn Any + Send>,
            _ctx: &mut ActorContext,
        ) -> ActorResult {
            if let Ok(BlockchainCommand::PreverifyCompleted(completed)) = envelope
                .downcast::<BlockchainCommand>()
                .map(|message| *message)
            {
                if let Some(sender) = self.completed.lock().take() {
                    let _ = sender.send(completed);
                }
            }
            Ok(())
        }
    }

    struct SenderProbe;

    #[async_trait]
    impl Actor for SenderProbe {
        async fn handle(
            &mut self,
            _envelope: Box<dyn Any + Send>,
            _ctx: &mut ActorContext,
        ) -> ActorResult {
            Ok(())
        }
    }

    struct SenderCaptureBlockchainProbe {
        expected_sender: ActorRef,
        observed: Arc<Mutex<Option<oneshot::Sender<bool>>>>,
    }

    #[async_trait]
    impl Actor for SenderCaptureBlockchainProbe {
        async fn handle(
            &mut self,
            envelope: Box<dyn Any + Send>,
            ctx: &mut ActorContext,
        ) -> ActorResult {
            if envelope.downcast::<BlockchainCommand>().is_ok() {
                let sender_matches = ctx
                    .sender()
                    .as_ref()
                    .is_some_and(|sender| sender == &self.expected_sender);
                if let Some(observer) = self.observed.lock().take() {
                    let _ = observer.send(sender_matches);
                }
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn transaction_router_handle_delivers_preverify_completed_to_blockchain() {
        let actor_system = ActorSystem::new("tx-router-test").expect("actor system");
        let (sender, receiver) = oneshot::channel();
        let completed = Arc::new(Mutex::new(Some(sender)));
        let probe = {
            let completed = completed.clone();
            actor_system
                .actor_of(
                    Props::new(move || BlockchainProbe {
                        completed: completed.clone(),
                    }),
                    "blockchain_probe",
                )
                .expect("probe actor")
        };
        let settings = Arc::new(ProtocolSettings::default());
        let handle = TransactionRouterHandle::spawn(settings.clone(), probe);
        let transaction = Transaction::new();
        let expected = transaction.verify_state_independent(&settings);

        handle
            .tell(TransactionRouterMessage::Preverify {
                transaction: transaction.clone(),
                relay: true,
            })
            .expect("send preverify");

        let completed = timeout(Duration::from_secs(2), receiver)
            .await
            .expect("preverify timeout")
            .expect("preverify result");

        assert_eq!(completed.relay, true);
        assert_eq!(completed.transaction.nonce(), transaction.nonce());
        assert_eq!(completed.result, expected);
        assert_eq!(completed.result, VerifyResult::Succeed);
    }

    #[tokio::test]
    async fn transaction_router_handle_preserves_sender_for_blockchain_replies() {
        let actor_system = ActorSystem::new("tx-router-sender-test").expect("actor system");
        let sender_ref = actor_system
            .actor_of(Props::new(|| SenderProbe), "sender_probe")
            .expect("sender probe");
        let (observer, receiver) = oneshot::channel();
        let observed = Arc::new(Mutex::new(Some(observer)));
        let blockchain = {
            let expected_sender = sender_ref.clone();
            let observed = observed.clone();
            actor_system
                .actor_of(
                    Props::new(move || SenderCaptureBlockchainProbe {
                        expected_sender: expected_sender.clone(),
                        observed: observed.clone(),
                    }),
                    "sender_capture_blockchain_probe",
                )
                .expect("blockchain probe")
        };
        let handle =
            TransactionRouterHandle::spawn(Arc::new(ProtocolSettings::default()), blockchain);

        handle
            .tell_from(
                TransactionRouterMessage::Preverify {
                    transaction: Transaction::new(),
                    relay: true,
                },
                Some(sender_ref),
            )
            .expect("send preverify");

        let sender_matches = timeout(Duration::from_secs(2), receiver)
            .await
            .expect("sender capture timeout")
            .expect("sender capture result");

        assert!(sender_matches);
    }
}
