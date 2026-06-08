//! Blockchain command / event handle.
//!
//! The blockchain is the only service in the runtime that is *command-shaped*
//! rather than *method-shaped*. Two reasons:
//!
//! 1. **Concurrency.** Multiple in-flight RPC requests, the consensus
//!    driver, and the network stack all want to import blocks at the
//!    same time. Funnelling every interaction through a single
//!    `mpsc::Sender<BlockchainCommand>` gives the blockchain a single
//!    owner that serialises state transitions in one place — exactly the
//!    property an actor system buys you, but expressed in plain
//!    `tokio::sync::mpsc` + `oneshot` so the call site is a normal
//!    `await`.
//!
//! 2. **Observability.** Every state transition is a typed command, so
//!    the command loop can log, trace, and instrument it without
//!    reaching into private state of an actor struct.
//!
//! The companion [`BlockchainEvent`] sum type is broadcast to every
//! subscriber on the [`broadcast::Sender`] returned by
//! [`BlockchainHandle::subscribe`]. The default broadcast capacity is
//! [`DEFAULT_EVENT_CAPACITY`] and is intentionally large to absorb burst
//! syncs without lagging the consensus driver.

use crate::errors::{ServiceError, ServiceResult};
use neo_payloads::Block;
use neo_primitives::UInt256;
use tokio::sync::{broadcast, mpsc, oneshot};

/// Default capacity of the [`BlockchainHandle::subscribe`] broadcast
/// channel. Sized to absorb a burst sync of several hundred blocks
/// without lagging the producer, while keeping the in-memory queue
/// bounded.
pub const DEFAULT_EVENT_CAPACITY: usize = 1024;

/// Default capacity of the command channel inside
/// [`BlockchainHandle::with_capacity`]. Sized to match the broadcast
/// capacity so a burst of imports does not block senders before the
/// broadcast queue fills up.
pub const DEFAULT_COMMAND_CAPACITY: usize = 1024;

/// Commands accepted by the blockchain service.
///
/// Each request / response command carries a `oneshot::Sender` so the
/// blockchain loop can reply without the caller having to spin up an
/// `Arc<Mutex<Option<T>>>` for the response.
#[derive(Debug)]
pub enum BlockchainCommand {
    /// Import a fully-validated block into the canonical chain.
    ImportBlock {
        /// The block to import.
        block: Block,
        /// Reply channel; the boolean indicates whether the import
        /// changed the canonical tip.
        reply: oneshot::Sender<ServiceResult<bool>>,
    },
    /// Fetch a block by its hash.
    GetBlock {
        /// Hash of the block to fetch.
        hash: UInt256,
        /// Reply channel.
        reply: oneshot::Sender<ServiceResult<Option<Block>>>,
    },
    /// Fetch a block by its height in the canonical chain.
    GetBlockByHeight {
        /// Height of the block to fetch.
        height: u32,
        /// Reply channel.
        reply: oneshot::Sender<ServiceResult<Option<Block>>>,
    },
    /// Return the current canonical tip height.
    GetHeight {
        /// Reply channel.
        reply: oneshot::Sender<ServiceResult<u32>>,
    },
    /// Graceful shutdown: ask the command loop to exit.
    ///
    /// Subscribers can still consume the [`broadcast::Receiver`] returned
    /// by [`BlockchainHandle::subscribe`] after shutdown to drain any
    /// final events.
    Shutdown,
}

/// Events broadcast on the [`BlockchainHandle::subscribe`] channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockchainEvent {
    /// A block was imported and became part of the canonical chain.
    Imported {
        /// Hash of the imported block.
        hash: UInt256,
        /// Height the block was assigned in the canonical chain.
        height: u32,
    },
    /// A previously imported block was reverted (re-org, rollback, …).
    Reverted {
        /// Hash of the reverted block.
        hash: UInt256,
        /// Height the block occupied before the revert.
        height: u32,
    },
    /// The canonical tip changed without a new block being imported
    /// (e.g. a fork-choice update chose a different chain tip).
    TipChanged {
        /// New tip hash.
        hash: UInt256,
        /// New tip height.
        height: u32,
    },
    /// The command loop has been shut down and no further events will
    /// be emitted.
    Shutdown,
}

/// Cheap-to-clone handle to a blockchain service.
///
/// A `BlockchainHandle` is what every other subsystem stores in its
/// state: it is `Clone`, `Send`, and `Sync`, and every method is a
/// normal `async fn` returning a [`ServiceResult`]. The handle is the
/// only stable public API of the blockchain — concrete
/// `BlockchainCore` / `RocksDbBlockchain` implementations are expected
/// to construct one of these via [`BlockchainHandle::with_capacity`]
/// and run the command loop themselves.
#[derive(Clone)]
pub struct BlockchainHandle {
    cmd_tx: mpsc::Sender<BlockchainCommand>,
    event_tx: broadcast::Sender<BlockchainEvent>,
}

impl std::fmt::Debug for BlockchainHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlockchainHandle")
            .field("cmd_capacity", &self.cmd_tx.capacity())
            .field("event_receivers", &self.event_tx.receiver_count())
            .finish()
    }
}

impl BlockchainHandle {
    /// Build a `(handle, command-receiver, event-sender)` triple.
    ///
    /// The caller is expected to spawn the blockchain command loop on
    /// the returned `mpsc::Receiver`, and to use the returned
    /// `broadcast::Sender` (or hand it to the loop) to publish events.
    /// Most callers should prefer [`BlockchainHandle::with_capacity`]
    /// when they do not need to drive the loop themselves.
    pub fn channel(
        cmd_capacity: usize,
        event_capacity: usize,
    ) -> (Self, mpsc::Receiver<BlockchainCommand>, broadcast::Sender<BlockchainEvent>) {
        let (cmd_tx, cmd_rx) = mpsc::channel(cmd_capacity);
        let (event_tx, _event_rx) = broadcast::channel(event_capacity);
        let handle = Self { cmd_tx, event_tx: event_tx.clone() };
        (handle, cmd_rx, event_tx)
    }

    /// Build a [`BlockchainHandle`] with default capacities and return
    /// the command receiver that the caller's blockchain loop should
    /// drive.
    pub fn with_capacity() -> (Self, mpsc::Receiver<BlockchainCommand>) {
        let (handle, cmd_rx, _event_tx) = Self::channel(DEFAULT_COMMAND_CAPACITY, DEFAULT_EVENT_CAPACITY);
        (handle, cmd_rx)
    }

    /// Subscribe to [`BlockchainEvent`]s.
    ///
    /// Each call returns an *independent* receiver; dropping the
    /// receiver automatically unregisters the subscription. The
    /// broadcast queue is sized at construction time via
    /// [`BlockchainHandle::channel`].
    pub fn subscribe(&self) -> broadcast::Receiver<BlockchainEvent> {
        self.event_tx.subscribe()
    }

    /// Import a block. Resolves to `Ok(true)` when the import changed
    /// the canonical tip.
    pub async fn import_block(&self, block: Block) -> ServiceResult<bool> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(BlockchainCommand::ImportBlock { block, reply: reply_tx })
            .await
            .map_err(|_| ServiceError::unavailable("blockchain command channel closed"))?;
        reply_rx
            .await
            .map_err(|_| ServiceError::unavailable("blockchain command reply dropped"))?
    }

    /// Fetch a block by hash.
    pub async fn get_block(&self, hash: &UInt256) -> ServiceResult<Option<Block>> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(BlockchainCommand::GetBlock { hash: *hash, reply: reply_tx })
            .await
            .map_err(|_| ServiceError::unavailable("blockchain command channel closed"))?;
        reply_rx
            .await
            .map_err(|_| ServiceError::unavailable("blockchain command reply dropped"))?
    }

    /// Fetch a block by canonical height.
    pub async fn get_block_by_height(&self, height: u32) -> ServiceResult<Option<Block>> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(BlockchainCommand::GetBlockByHeight { height, reply: reply_tx })
            .await
            .map_err(|_| ServiceError::unavailable("blockchain command channel closed"))?;
        reply_rx
            .await
            .map_err(|_| ServiceError::unavailable("blockchain command reply dropped"))?
    }

    /// Current canonical tip height.
    pub async fn get_height(&self) -> ServiceResult<u32> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(BlockchainCommand::GetHeight { reply: reply_tx })
            .await
            .map_err(|_| ServiceError::unavailable("blockchain command channel closed"))?;
        reply_rx
            .await
            .map_err(|_| ServiceError::unavailable("blockchain command reply dropped"))?
    }

    /// Request graceful shutdown of the command loop.
    ///
    /// Returns `Ok` once the command was accepted by the channel; the
    /// loop drains in-flight commands before exiting. The caller can
    /// still hold the handle and call the request/response methods
    /// after shutdown — they will all return
    /// [`ServiceError::ServiceUnavailable`] because the receiver has
    /// been dropped.
    pub async fn shutdown(&self) -> ServiceResult<()> {
        self.cmd_tx
            .send(BlockchainCommand::Shutdown)
            .await
            .map_err(|_| ServiceError::unavailable("blockchain command channel closed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_payloads::Block;

    #[tokio::test]
    async fn get_height_returns_service_unavailable_after_drop() {
        let (handle, rx) = BlockchainHandle::with_capacity();
        drop(rx);
        let err = handle.get_height().await.expect_err("should fail");
        assert!(matches!(err, ServiceError::ServiceUnavailable(_)));
    }

    #[tokio::test]
    async fn import_block_round_trip() {
        // Drive a tiny command loop that just acks every command.
        let (handle, mut rx) = BlockchainHandle::with_capacity();
        let event_tx = handle.event_tx.clone();

        tokio::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    BlockchainCommand::GetHeight { reply } => {
                        let _ = reply.send(Ok(42));
                    }
                    BlockchainCommand::ImportBlock { block, reply } => {
                        let hash = block.hash();
                        let _ = reply.send(Ok(true));
                        let _ = event_tx.send(BlockchainEvent::Imported { hash, height: 42 });
                    }
                    BlockchainCommand::Shutdown => break,
                    _ => {}
                }
            }
        });

        assert_eq!(handle.get_height().await.unwrap(), 42);
        let block = Block::new();
        let changed = handle.import_block(block).await.unwrap();
        assert!(changed);
        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn subscribe_yields_broadcast_events() {
        let (handle, mut rx) = BlockchainHandle::with_capacity();
        let event_tx = handle.event_tx.clone();
        let mut sub = handle.subscribe();

        tokio::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                if let BlockchainCommand::Shutdown = cmd {
                    break;
                }
            }
        });

        event_tx
            .send(BlockchainEvent::TipChanged {
                hash: UInt256::default(),
                height: 1,
            })
            .unwrap();

        let ev = sub.recv().await.unwrap();
        assert_eq!(
            ev,
            BlockchainEvent::TipChanged {
                hash: UInt256::default(),
                height: 1,
            }
        );

        handle.shutdown().await.unwrap();
    }
}
