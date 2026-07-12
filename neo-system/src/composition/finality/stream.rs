//! Acknowledged bounded stream implementation.

use std::marker::PhantomData;
use std::sync::Arc;

use neo_blockchain::FinalizedBlock;
use neo_storage::CacheRead;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

use super::FinalizedBlockConsumer;

/// Default number of finalized notifications that may wait for the consumer.
///
/// The canonical writer currently awaits every acknowledgement, so normal
/// operation has at most one in flight. The extra capacity absorbs controlled
/// producer handoff during shutdown and future batch publication without
/// making memory growth unbounded.
pub const DEFAULT_FINALITY_CAPACITY: usize = 64;

/// Finalized stream publication or worker failure.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum FinalizedBlockStreamError {
    /// The consumer task exited before accepting the notification.
    #[error("finalized block stream is closed")]
    Closed,
    /// The worker exited after accepting a notification but before acknowledging it.
    #[error("finalized block acknowledgement was dropped")]
    AcknowledgementDropped,
    /// The configured consumer rejected a finalized notification.
    #[error("finalized block consumer failed: {0}")]
    Consumer(String),
    /// The blocking projection worker panicked or was cancelled.
    #[error("finalized block worker failed: {0}")]
    Worker(String),
}

struct Delivery<B>
where
    B: CacheRead,
{
    finalized: FinalizedBlock<B>,
    acknowledgement: oneshot::Sender<Result<(), String>>,
}

/// Cloneable producer capability for the canonical writer.
pub struct FinalizedBlockHandle<B>
where
    B: CacheRead,
{
    sender: mpsc::Sender<Delivery<B>>,
}

impl<B> Clone for FinalizedBlockHandle<B>
where
    B: CacheRead,
{
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
        }
    }
}

impl<B> std::fmt::Debug for FinalizedBlockHandle<B>
where
    B: CacheRead,
{
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FinalizedBlockHandle")
            .field("capacity", &self.sender.capacity())
            .field("closed", &self.sender.is_closed())
            .finish()
    }
}

impl<B> FinalizedBlockHandle<B>
where
    B: CacheRead,
{
    /// Publishes one notification and waits until its consumer acknowledges it.
    pub async fn publish(
        &self,
        finalized: FinalizedBlock<B>,
    ) -> Result<(), FinalizedBlockStreamError> {
        let (acknowledgement, received) = oneshot::channel();
        self.sender
            .send(Delivery {
                finalized,
                acknowledgement,
            })
            .await
            .map_err(|_| FinalizedBlockStreamError::Closed)?;
        received
            .await
            .map_err(|_| FinalizedBlockStreamError::AcknowledgementDropped)?
            .map_err(FinalizedBlockStreamError::Consumer)
    }
}

/// Single-consumer side of the finalized notification stream.
pub struct FinalizedBlockStream<B, C>
where
    B: CacheRead,
    C: FinalizedBlockConsumer<B>,
{
    receiver: mpsc::Receiver<Delivery<B>>,
    consumer: Arc<C>,
}

impl<B, C> FinalizedBlockStream<B, C>
where
    B: CacheRead,
    C: FinalizedBlockConsumer<B>,
{
    /// Runs until every producer is dropped or a consumer fails.
    pub async fn run(mut self) -> Result<(), FinalizedBlockStreamError> {
        while let Some(delivery) = self.receiver.recv().await {
            let consumer = Arc::clone(&self.consumer);
            let finalized = delivery.finalized;
            let result = tokio::task::spawn_blocking(move || consumer.consume(&finalized))
                .await
                .map_err(|error| FinalizedBlockStreamError::Worker(error.to_string()))?;
            let acknowledgement = delivery.acknowledgement;
            match result {
                Ok(()) => {
                    let _ = acknowledgement.send(Ok(()));
                }
                Err(error) => {
                    let _ = acknowledgement.send(Err(error.clone()));
                    return Err(FinalizedBlockStreamError::Consumer(error));
                }
            }
        }
        Ok(())
    }
}

/// Factory that binds queue policy to a concrete consumer type.
#[derive(Debug, Clone, Copy)]
pub struct FinalizedBlockStreamFactory {
    capacity: usize,
}

impl FinalizedBlockStreamFactory {
    /// Creates a factory with a non-zero bounded capacity.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        assert!(
            capacity > 0,
            "finalized block stream capacity must be non-zero"
        );
        Self { capacity }
    }

    /// Creates the canonical producer/consumer pair.
    #[must_use]
    pub fn create<B, C>(
        self,
        consumer: Arc<C>,
    ) -> (FinalizedBlockHandle<B>, FinalizedBlockStream<B, C>)
    where
        B: CacheRead,
        C: FinalizedBlockConsumer<B>,
    {
        let (sender, receiver) = mpsc::channel(self.capacity);
        (
            FinalizedBlockHandle { sender },
            FinalizedBlockStream { receiver, consumer },
        )
    }

    /// Retains the selected cache backing in generic factory contexts.
    #[must_use]
    pub const fn for_backing<B>(self) -> FinalizedBlockStreamFactoryFor<B>
    where
        B: CacheRead,
    {
        FinalizedBlockStreamFactoryFor {
            factory: self,
            marker: PhantomData,
        }
    }
}

impl Default for FinalizedBlockStreamFactory {
    fn default() -> Self {
        Self::new(DEFAULT_FINALITY_CAPACITY)
    }
}

/// Cache-backing-specific factory adapter used by composition roots.
#[derive(Debug, Clone, Copy)]
pub struct FinalizedBlockStreamFactoryFor<B>
where
    B: CacheRead,
{
    factory: FinalizedBlockStreamFactory,
    marker: PhantomData<fn() -> B>,
}

impl<B> FinalizedBlockStreamFactoryFor<B>
where
    B: CacheRead,
{
    /// Creates a stream without repeating the backing type at the call site.
    #[must_use]
    pub fn create<C>(
        self,
        consumer: Arc<C>,
    ) -> (FinalizedBlockHandle<B>, FinalizedBlockStream<B, C>)
    where
        C: FinalizedBlockConsumer<B>,
    {
        self.factory.create(consumer)
    }
}

#[cfg(test)]
#[path = "../../tests/composition/finality.rs"]
mod tests;
