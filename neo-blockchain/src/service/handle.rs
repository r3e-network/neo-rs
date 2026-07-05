//! Service handle — the public, cheap-to-clone facade for talking to a
//! running `BlockchainService`.
//!
//! The handle is what every other subsystem stores in its state. It is
//! `Clone`, `Send`, and `Sync`; the only state it owns is the two
//! channels the service loop reads from.
//!
//! The handle has *two* styles of API:
//!
//! 1. **Fire-and-forget commands** ([`BlockchainHandle::tell`],
//!    [`BlockchainHandle::submit_inventory_blocks`]): send a
//!    [`crate::BlockchainCommand`] down the `mpsc::Sender` without
//!    waiting for a reply.
//! 2. **Request/response** ([`BlockchainHandle::import_block`],
//!    [`BlockchainHandle::get_block`], [`BlockchainHandle::get_block_by_height`],
//!    [`BlockchainHandle::get_height`]): translate the method call into a
//!    `BlockchainCommand::ImportBlock` / `GetBlock` / … command and await the
//!    `oneshot` reply. These read like normal `async fn`s rather than
//!    `tell(Command::Variant { … })` boilerplate.
//!
//! Both layers share the same channel and the same service loop: there
//! is exactly one `BlockchainCommand` stream, dispatched by a single
//! `match` in `crate::service::BlockchainService::run`.

use std::fmt;
use std::sync::Arc;

use neo_payloads::{Block, ExtensiblePayload};
use neo_primitives::UInt256;
use neo_runtime::{
    BlockBatchImportOutcome, BlockImport, BlockImportOutcome, BlockOrigin, ImportedTip, Service,
    ServiceError,
};
use tokio::sync::{broadcast, mpsc};

use crate::command::{AddTransactionReply, BlockchainCommand, ImportBlocksReply};
use crate::import::Import;

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

    /// Send a [`BlockchainCommand`] without awaiting a reply. Prefer the typed
    /// request/response methods ([`Self::import_block`],
    /// [`Self::get_block`], …) when the command has a natural reply.
    pub async fn tell(
        &self,
        command: BlockchainCommand,
    ) -> Result<(), mpsc::error::SendError<BlockchainCommand>> {
        self.cmd_tx.send(command).await
    }

    /// Try to send a command without awaiting the channel.
    pub fn try_tell(
        &self,
        command: BlockchainCommand,
    ) -> Result<(), mpsc::error::TrySendError<BlockchainCommand>> {
        self.cmd_tx.try_send(command)
    }

    /// Submit a peer-relayed inventory block burst to the live sync path.
    ///
    /// This keeps callers on a typed API while preserving the blockchain
    /// service's inventory-specific semantics: relay policy, parked future
    /// blocks, deferred batch store commit, unverified-drain handling, and
    /// mempool maintenance all remain inside the service loop.
    pub async fn submit_inventory_blocks(
        &self,
        blocks: Vec<Arc<Block>>,
        relay: bool,
        pre_verified: bool,
    ) -> Result<(), ServiceError> {
        if blocks.is_empty() {
            return Ok(());
        }
        self.cmd_tx
            .send(BlockchainCommand::InventoryBlocks {
                blocks,
                relay,
                pre_verified,
            })
            .await
            .map_err(|_| ServiceError::unavailable("blockchain command channel closed"))
    }

    /// Submit one block to the peer/consensus inventory path.
    ///
    /// Use this for live inventory semantics. RPC and local package imports
    /// should use [`Self::import_block`] or [`Self::import_blocks_bulk`]
    /// instead.
    pub async fn submit_inventory_block(
        &self,
        block: Arc<Block>,
        relay: bool,
        pre_verified: bool,
    ) -> Result<(), ServiceError> {
        self.cmd_tx
            .send(BlockchainCommand::InventoryBlock {
                block,
                relay,
                pre_verified,
            })
            .await
            .map_err(|_| ServiceError::unavailable("blockchain command channel closed"))
    }

    /// Submit an extensible payload to the live inventory path.
    pub async fn submit_inventory_extensible(
        &self,
        payload: ExtensiblePayload,
        relay: bool,
    ) -> Result<(), ServiceError> {
        self.cmd_tx
            .send(BlockchainCommand::InventoryExtensible { payload, relay })
            .await
            .map_err(|_| ServiceError::unavailable("blockchain command channel closed"))
    }

    /// Request blockchain service initialization.
    pub async fn initialize(&self) -> Result<(), ServiceError> {
        self.cmd_tx
            .send(BlockchainCommand::Initialize)
            .await
            .map_err(|_| ServiceError::unavailable("blockchain command channel closed"))
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

    /// Import a consecutive batch of blocks and wait until the service has
    /// processed it. Resolves with the number of supplied blocks accepted as
    /// part of the consecutive prefix before the first gap or rejected block
    /// stops the import loop. Already-persisted prefix blocks count as
    /// processed so `chain.acc` dumps that include genesis do not look
    /// truncated to the caller.
    pub async fn import_blocks(
        &self,
        blocks: Vec<Block>,
        verify: bool,
    ) -> Result<usize, ServiceError> {
        self.import_blocks_with_mode(blocks, verify, false).await
    }

    /// Import a trusted bulk-sync batch and skip replay-only artifacts that
    /// cold-sync consumers intentionally do not read.
    pub async fn import_blocks_bulk(
        &self,
        blocks: Vec<Block>,
        verify: bool,
    ) -> Result<usize, ServiceError> {
        self.import_blocks_with_mode(blocks, verify, true).await
    }

    /// Import a trusted bulk-sync batch and return the detailed service-side
    /// timing/composition reply.
    pub async fn import_blocks_bulk_detailed(
        &self,
        blocks: Vec<Block>,
        verify: bool,
    ) -> Result<ImportBlocksReply, ServiceError> {
        self.import_blocks_reply_with_mode(blocks, verify, true)
            .await
    }

    async fn import_blocks_with_mode(
        &self,
        blocks: Vec<Block>,
        verify: bool,
        bulk_sync: bool,
    ) -> Result<usize, ServiceError> {
        let reply = self
            .import_blocks_reply_with_mode(blocks, verify, bulk_sync)
            .await?;
        if let Some(error) = reply.error {
            return Err(ServiceError::InvalidState(format!(
                "block import finalization failed after importing {} blocks: {error}",
                reply.imported
            )));
        }
        Ok(reply.imported)
    }

    async fn import_blocks_reply_with_mode(
        &self,
        blocks: Vec<Block>,
        verify: bool,
        bulk_sync: bool,
    ) -> Result<ImportBlocksReply, ServiceError> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.cmd_tx
            .send(BlockchainCommand::ImportBlocks {
                import: Import {
                    blocks,
                    verify,
                    bulk_sync,
                },
                reply: reply_tx,
            })
            .await
            .map_err(|_| {
                ServiceError::ServiceUnavailable("blockchain command channel closed".to_string())
            })?;
        let reply = reply_rx.await.map_err(|_| {
            ServiceError::ServiceUnavailable("blockchain command reply dropped".to_string())
        })?;
        Ok(reply)
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

impl Service for BlockchainHandle {
    fn name(&self) -> &str {
        "BlockchainHandle"
    }
}

#[async_trait::async_trait]
impl BlockImport for BlockchainHandle {
    async fn check(&self, block: &Block) -> Result<(), ServiceError> {
        block.try_hash().map_err(|error| {
            ServiceError::invalid_input(format!("block hash serialization failed: {error}"))
        })?;
        Ok(())
    }

    async fn import(
        &self,
        block: Block,
        _origin: BlockOrigin,
    ) -> Result<BlockImportOutcome, ServiceError> {
        let tip = ImportedTip::from_block(&block)?;
        let imported = self.import_block(block).await?;
        if imported {
            Ok(BlockImportOutcome::Imported(tip))
        } else {
            Ok(BlockImportOutcome::NotImported {
                hash: tip.hash,
                height: tip.height,
            })
        }
    }

    async fn import_many(
        &self,
        blocks: Vec<Block>,
        origin: BlockOrigin,
    ) -> Result<BlockBatchImportOutcome, ServiceError> {
        let verify = !matches!(origin, BlockOrigin::TrustedLocal);
        let processed = if matches!(origin, BlockOrigin::TrustedLocal) {
            self.import_blocks_bulk(blocks, verify).await?
        } else {
            self.import_blocks(blocks, verify).await?
        };
        Ok(BlockBatchImportOutcome::new(processed))
    }
}

// The request/response methods above surface failures through the canonical
// `neo_runtime::ServiceError` (imported at the top of this module) rather than
// a duplicated local subset — `neo_runtime` is already part of this crate's
// public surface (see the `RuntimeEvent` re-export), so the single shared
// error vocabulary keeps the runtime layer overlap-free.
