//! Internal task handles for the neo_system module.
//!
//! This module contains lightweight typed task handles that process internal
//! Neo system work without routing everything through boxed actor messages.
//!
//! # Tasks
//!
//! - `TransactionRouterHandle` - Routes and pre-verifies incoming transactions

use std::sync::Arc;

use neo_actors::{ActorRef, ActorRuntimeError, ActorRuntimeResult};
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::warn;

use crate::blockchain::{BlockchainCommand, BlockchainHandle};
use crate::mempool::TransactionRouter as MempoolTransactionRouter;
use crate::network::p2p::payloads::transaction::Transaction;
use neo_protocol::protocol_settings::ProtocolSettings;

const TX_ROUTER_MAILBOX_CAPACITY: usize = 65_536;
/// Bounds speculative CPU work and queued results without delaying the first
/// transaction: only work already present in the mailbox is added to a batch.
const TX_ROUTER_PREVERIFY_BATCH_SIZE: usize = 64;

struct TransactionRouterEnvelope {
    transaction: Transaction,
    relay: bool,
    sender: Option<ActorRef>,
}

/// Simple transaction router for state-independent verification
struct TransactionRouter {
    inner: MempoolTransactionRouter,
}

impl TransactionRouter {
    fn new(settings: ProtocolSettings) -> Self {
        Self {
            inner: MempoolTransactionRouter::new(settings),
        }
    }

    fn convert(
        completed: crate::mempool::PreverifyCompleted,
    ) -> crate::blockchain::PreverifyCompleted {
        let hash = completed.transaction.hash();
        crate::blockchain::PreverifyCompleted {
            hash,
            transaction: completed.transaction,
            result: completed.result,
            relay: completed.relay,
        }
    }

    fn preverify_batch(
        &self,
        envelopes: Vec<TransactionRouterEnvelope>,
    ) -> Vec<(Option<ActorRef>, crate::blockchain::PreverifyCompleted)> {
        let (senders, transactions): (Vec<_>, Vec<_>) = envelopes
            .into_iter()
            .map(|envelope| (envelope.sender, (envelope.transaction, envelope.relay)))
            .unzip();

        self.inner
            .preverify_batch(transactions)
            .into_iter()
            .map(Self::convert)
            .zip(senders)
            .map(|(completed, sender)| (sender, completed))
            .collect()
    }
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
    pub(crate) fn spawn(
        settings: Arc<ProtocolSettings>,
        blockchain: impl Into<BlockchainHandle>,
    ) -> Self {
        let (sender, receiver) = mpsc::channel(TX_ROUTER_MAILBOX_CAPACITY);
        let router = TransactionRouter::new((*settings).clone());
        let task = Arc::new(tokio::spawn(run_transaction_router(
            router,
            blockchain.into(),
            receiver,
        )));

        Self { sender, task }
    }

    /// Stops the router worker task.
    pub fn abort(&self) {
        self.task.abort();
    }

    /// Enqueues transaction pre-verification without waiting for mailbox capacity.
    pub fn try_enqueue_preverify(
        &self,
        transaction: Transaction,
        relay: bool,
    ) -> ActorRuntimeResult<()> {
        self.try_enqueue_preverify_from(transaction, relay, None)
    }

    /// Enqueues transaction pre-verification with an optional actor sender for relay responses.
    pub fn try_enqueue_preverify_from(
        &self,
        transaction: Transaction,
        relay: bool,
        sender: Option<ActorRef>,
    ) -> ActorRuntimeResult<()> {
        self.sender
            .try_send(TransactionRouterEnvelope {
                transaction,
                relay,
                sender,
            })
            .map_err(|error| ActorRuntimeError::send(error.to_string()))
    }

    /// Enqueues transaction pre-verification with backpressure and an optional actor sender.
    pub async fn enqueue_preverify_from(
        &self,
        transaction: Transaction,
        relay: bool,
        sender: Option<ActorRef>,
    ) -> ActorRuntimeResult<()> {
        self.sender
            .send(TransactionRouterEnvelope {
                transaction,
                relay,
                sender,
            })
            .await
            .map_err(|error| ActorRuntimeError::send(error.to_string()))
    }
}

async fn run_transaction_router(
    router: TransactionRouter,
    blockchain: BlockchainHandle,
    mut receiver: mpsc::Receiver<TransactionRouterEnvelope>,
) {
    while let Some(first) = receiver.recv().await {
        let mut envelopes = Vec::with_capacity(TX_ROUTER_PREVERIFY_BATCH_SIZE);
        envelopes.push(first);
        for _ in 1..TX_ROUTER_PREVERIFY_BATCH_SIZE {
            match receiver.try_recv() {
                Ok(envelope) => envelopes.push(envelope),
                Err(mpsc::error::TryRecvError::Empty | mpsc::error::TryRecvError::Disconnected) => {
                    break;
                }
            }
        }

        // Parallel verification is confined to pure, state-independent work.
        // Delivery remains sequential and in mailbox order; the blockchain actor
        // still performs canonical state-dependent validation and pool admission.
        for (sender, completed) in router.preverify_batch(envelopes) {
            if let Err(error) =
                blockchain.tell_from(BlockchainCommand::PreverifyCompleted(completed), sender)
            {
                warn!(
                    target: "neo",
                    %error,
                    "failed to deliver preverify result to blockchain actor"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockchain::PreverifyCompleted;
    use async_trait::async_trait;
    use neo_actors::{Actor, ActorContext, ActorResult, ActorSystem, Props};
    use neo_ledger::VerifyResult;
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
                && let Some(sender) = self.completed.lock().take()
            {
                let _ = sender.send(completed);
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
        let handle = TransactionRouterHandle::spawn(settings.clone(), BlockchainHandle::new(probe));
        let transaction = Transaction::new();
        let expected = neo_ledger::verify_transaction_state_independent(&transaction, &settings);

        handle
            .try_enqueue_preverify(transaction.clone(), true)
            .expect("send preverify");

        let completed = timeout(Duration::from_secs(2), receiver)
            .await
            .expect("preverify timeout")
            .expect("preverify result");

        assert!(completed.relay);
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
        let handle = TransactionRouterHandle::spawn(
            Arc::new(ProtocolSettings::default()),
            BlockchainHandle::new(blockchain),
        );

        handle
            .try_enqueue_preverify_from(Transaction::new(), true, Some(sender_ref))
            .expect("send preverify");

        let sender_matches = timeout(Duration::from_secs(2), receiver)
            .await
            .expect("sender capture timeout")
            .expect("sender capture result");

        assert!(sender_matches);
    }

    #[test]
    fn transaction_router_batch_preserves_mailbox_order_and_relay_flags() {
        let router = TransactionRouter::new(ProtocolSettings::default());
        let envelopes = (0..TX_ROUTER_PREVERIFY_BATCH_SIZE)
            .map(|nonce| {
                let mut transaction = Transaction::new();
                transaction.set_nonce(nonce as u32);
                TransactionRouterEnvelope {
                    transaction,
                    relay: nonce % 2 == 0,
                    sender: None,
                }
            })
            .collect();

        let completed = router.preverify_batch(envelopes);

        assert_eq!(completed.len(), TX_ROUTER_PREVERIFY_BATCH_SIZE);
        for (nonce, (sender, result)) in completed.into_iter().enumerate() {
            assert!(sender.is_none());
            assert_eq!(result.transaction.nonce(), nonce as u32);
            assert_eq!(result.relay, nonce % 2 == 0);
            assert_eq!(result.result, VerifyResult::Succeed);
        }
    }
}
