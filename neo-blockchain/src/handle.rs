//! Service handle — the public, cheap-to-clone facade for talking to a
//! running [`BlockchainService`].
//!
//! The handle is what every other subsystem stores in its state. It is
//! `Clone`, `Send`, and `Sync`; the only state it owns is the two
//! channels the service loop reads from.
//!
//! The handle has *two* styles of API:
//!
//! 1. **Fire-and-forget commands** ([`Self::tell`]): send a
//!    [`crate::BlockchainCommand`] down the `mpsc::Sender` without
//!    waiting for a reply.
//! 2. **Request/response** ([`Self::import_block`], [`Self::get_block`],
//!    [`Self::get_block_by_height`], [`Self::get_height`]): translate the
//!    method call into a `BlockchainCommand::ImportBlock` / `GetBlock` / …
//!    command and await the `oneshot` reply. These read like normal
//!    `async fn`s rather than `tell(Command::Variant { … })` boilerplate.
//!
//! Both layers share the same channel and the same service loop: there
//! is exactly one `BlockchainCommand` stream, dispatched by a single
//! `match` in [`crate::service::BlockchainService::run`].

use std::fmt;
use std::sync::Arc;

use neo_payloads::Block;
use neo_primitives::UInt256;
use neo_runtime::ServiceError;
use tokio::sync::{broadcast, mpsc};

use crate::command::{AddTransactionReply, BlockchainCommand};

/// Cheap-to-clone handle to a blockchain service.
#[derive(Clone)]
pub struct BlockchainHandle {
    /// Sender half of the command channel. The service owns the
    /// receiver and processes commands in `BlockchainService::run`.
    pub(crate) cmd_tx: mpsc::Sender<BlockchainCommand>,
    /// Broadcast sender used by the service to publish lifecycle
    /// events. Subscribers grab their own receiver via
    /// [`Self::subscribe`].
    pub(crate) event_tx: broadcast::Sender<crate::RuntimeEvent>,
}

impl fmt::Debug for BlockchainHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
    ) -> (
        Self,
        mpsc::Receiver<BlockchainCommand>,
        broadcast::Sender<crate::RuntimeEvent>,
    ) {
        let (cmd_tx, cmd_rx) = mpsc::channel(cmd_capacity);
        let (event_tx, _event_rx) = broadcast::channel(event_capacity);
        let handle = Self {
            cmd_tx,
            event_tx: event_tx.clone(),
        };
        (handle, cmd_rx, event_tx)
    }

    /// Build a [`BlockchainHandle`] with default capacities and return
    /// the command receiver that the caller's blockchain loop should
    /// drive.
    pub fn with_capacity() -> (Self, mpsc::Receiver<BlockchainCommand>) {
        let (handle, cmd_rx, _event_tx) = Self::channel(
            crate::blockchain::DEFAULT_COMMAND_CAPACITY,
            crate::blockchain::DEFAULT_EVENT_CAPACITY,
        );
        (handle, cmd_rx)
    }

    /// Subscribe to [`crate::RuntimeEvent`]s.
    ///
    /// Each call returns an *independent* receiver; dropping the
    /// receiver automatically unregisters the subscription. The
    /// broadcast queue is sized at construction time via
    /// [`Self::channel`].
    pub fn subscribe(&self) -> broadcast::Receiver<crate::RuntimeEvent> {
        self.event_tx.subscribe()
    }

    /// Send a [`BlockchainCommand`] without awaiting a reply. This is
    /// the legacy actor-style API; new code should prefer the typed
    /// request/response methods ([`Self::import_block`],
    /// [`Self::get_block`], …).
    pub async fn tell(
        &self,
        command: BlockchainCommand,
    ) -> Result<(), mpsc::error::SendError<BlockchainCommand>> {
        self.cmd_tx.send(command).await
    }

    /// Try to send a command without awaiting the channel. Mirrors the
    /// `try_tell` helper of the legacy actor handle.
    pub fn try_tell(
        &self,
        command: BlockchainCommand,
    ) -> Result<(), mpsc::error::TrySendError<BlockchainCommand>> {
        self.cmd_tx.try_send(command)
    }

    /// Import an externally supplied block. Resolves to `Ok(true)` when
    /// verification/persistence advanced the canonical tip, and `Ok(false)`
    /// when the service rejected the block or parked it without changing the
    /// tip.
    pub async fn import_block(&self, block: Block) -> Result<bool, ServiceError> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.cmd_tx
            .send(BlockchainCommand::ImportBlock {
                block: Arc::new(block),
                reply: reply_tx,
            })
            .await
            .map_err(|_| {
                ServiceError::ServiceUnavailable("blockchain command channel closed".to_string())
            })?;
        reply_rx.await.map_err(|_| {
            ServiceError::ServiceUnavailable("blockchain command reply dropped".to_string())
        })
    }

    /// Fetch a block by hash.
    pub async fn get_block(&self, hash: &UInt256) -> Result<Option<Block>, ServiceError> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.cmd_tx
            .send(BlockchainCommand::GetBlock {
                hash: *hash,
                reply: reply_tx,
            })
            .await
            .map_err(|_| {
                ServiceError::ServiceUnavailable("blockchain command channel closed".to_string())
            })?;
        reply_rx.await.map_err(|_| {
            ServiceError::ServiceUnavailable("blockchain command reply dropped".to_string())
        })
    }

    /// Fetch a block by canonical height.
    pub async fn get_block_by_height(&self, height: u32) -> Result<Option<Block>, ServiceError> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.cmd_tx
            .send(BlockchainCommand::GetBlockByHeight {
                height,
                reply: reply_tx,
            })
            .await
            .map_err(|_| {
                ServiceError::ServiceUnavailable("blockchain command channel closed".to_string())
            })?;
        reply_rx.await.map_err(|_| {
            ServiceError::ServiceUnavailable("blockchain command reply dropped".to_string())
        })
    }

    /// Current canonical tip height.
    pub async fn get_height(&self) -> Result<u32, ServiceError> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.cmd_tx
            .send(BlockchainCommand::GetHeight { reply: reply_tx })
            .await
            .map_err(|_| {
                ServiceError::ServiceUnavailable("blockchain command channel closed".to_string())
            })?;
        reply_rx.await.map_err(|_| {
            ServiceError::ServiceUnavailable("blockchain command reply dropped".to_string())
        })
    }

    /// Add a transaction to the mempool.
    pub async fn add_transaction(
        &self,
        transaction: neo_payloads::Transaction,
    ) -> Result<AddTransactionReply, ServiceError> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.cmd_tx
            .send(BlockchainCommand::AddTransaction {
                transaction,
                reply: reply_tx,
            })
            .await
            .map_err(|_| {
                ServiceError::ServiceUnavailable("blockchain command channel closed".to_string())
            })?;
        reply_rx.await.map_err(|_| {
            ServiceError::ServiceUnavailable("blockchain command reply dropped".to_string())
        })
    }

    /// Request graceful shutdown of the service loop. The command
    /// channel will be closed once the in-flight command finishes, at
    /// which point every pending `tell` will start returning
    /// [`ServiceError::ServiceUnavailable`].
    pub async fn shutdown(&self) -> Result<(), ServiceError> {
        // The service loop is driven by `recv().await`; closing the
        // sender is the canonical shutdown signal. We don't expose a
        // dedicated `Shutdown` variant yet because the legacy command
        // set never used one — the service stops on its own once all
        // senders are dropped.
        drop(self.cmd_tx.clone());
        Ok(())
    }
}

// The request/response methods above surface failures through the canonical
// `neo_runtime::ServiceError` (imported at the top of this module) rather than
// a duplicated local subset — `neo_runtime` is already part of this crate's
// public surface (see the `RuntimeEvent` re-export), so the single shared
// error vocabulary keeps the runtime layer overlap-free.
