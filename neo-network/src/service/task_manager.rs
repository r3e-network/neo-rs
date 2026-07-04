//! `TaskManagerService` — reth-style sync task orchestrator.
//!
//! The third of the three services that make up the reth-style
//! network host. The task manager tracks in-flight inventory
//! requests, schedules new requests, and notifies the local node
//! when an inventory item has been fully fetched from a peer.
//!
//! The task manager owns the in-memory set of active sync tasks and exposes a
//! typed command handle for adding, cancelling, completing, and listing them.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use neo_primitives::UInt256;

use crate::error::{NetworkError, NetworkResult};
use crate::event::NetworkEvent;
use crate::peer_id::PeerId;

/// Stable identifier for a single in-flight sync task.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TaskId(pub u64);

impl TaskId {
    /// Allocate a fresh globally-unique task id.
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "task:{}", self.0)
    }
}

/// What kind of inventory a task is fetching.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncTaskKind {
    /// Fetching a single block.
    Block,
    /// Fetching a single transaction.
    Transaction,
    /// Fetching a block header.
    Header,
    /// Fetching a batch of blocks by index range.
    BlockIndexBatch,
}

/// What the task manager will ask the remote node to do.
#[derive(Clone, Debug)]
pub enum SyncTask {
    /// Fetch a single block by hash.
    FetchBlock {
        /// Block hash to request.
        hash: UInt256,
        /// Kind of block-oriented request.
        kind: SyncTaskKind,
    },
    /// Fetch a single transaction by hash.
    FetchTransaction {
        /// Transaction hash to request.
        hash: UInt256,
    },
    /// Fetch headers in `[start, start + count)`.
    FetchHeaders {
        /// First header index to request.
        start: u32,
        /// Number of headers to request.
        count: u16,
    },
    /// Fetch blocks by index range.
    FetchBlocksByIndex {
        /// First block index to request.
        start: u32,
        /// Number of blocks to request.
        count: u32,
    },
}

/// Per-task command enum sent down the
/// `mpsc::Sender<TaskManagerCommand>` half of the task-manager
/// channel.
#[derive(Debug)]
pub enum TaskManagerCommand {
    /// Add a new sync task. The reply resolves with the task id.
    AddTask {
        /// The task to add.
        task: SyncTask,
        /// Reply channel.
        reply: oneshot::Sender<NetworkResult<TaskId>>,
    },
    /// Cancel an in-flight task.
    CancelTask {
        /// Identifier of the task to cancel.
        task_id: TaskId,
    },
    /// Mark a task as completed by a particular peer.
    CompleteTask {
        /// Identifier of the task.
        task_id: TaskId,
        /// Identifier of the peer that fulfilled the task.
        peer_id: PeerId,
    },
    /// List the in-flight task ids.
    ActiveTasks {
        /// Reply channel.
        reply: oneshot::Sender<Vec<TaskId>>,
    },
    /// Request graceful shutdown.
    Shutdown,
}

/// Cheap-to-clone handle to a running [`TaskManagerService`] task.
#[derive(Clone)]
pub struct TaskManagerHandle {
    cmd_tx: mpsc::Sender<TaskManagerCommand>,
    event_tx: broadcast::Sender<NetworkEvent>,
}

impl fmt::Debug for TaskManagerHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TaskManagerHandle")
            .field("cmd_capacity", &self.cmd_tx.capacity())
            .field("event_receivers", &self.event_tx.receiver_count())
            .finish()
    }
}

impl TaskManagerHandle {
    /// Add a sync task. Resolves with the new task's id once the
    /// task has been registered.
    pub async fn add_task(&self, task: SyncTask) -> NetworkResult<TaskId> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(TaskManagerCommand::AddTask {
                task,
                reply: reply_tx,
            })
            .await
            .map_err(|_| NetworkError::LocalShuttingDown)?;
        let reply = reply_rx
            .await
            .map_err(|_| NetworkError::LocalShuttingDown)??;
        Ok(reply)
    }

    /// Cancel a task.
    pub async fn cancel_task(&self, task_id: TaskId) -> NetworkResult<()> {
        self.cmd_tx
            .send(TaskManagerCommand::CancelTask { task_id })
            .await
            .map_err(|_| NetworkError::LocalShuttingDown)
    }

    /// Mark a task as completed.
    pub async fn complete_task(&self, task_id: TaskId, peer_id: PeerId) -> NetworkResult<()> {
        self.cmd_tx
            .send(TaskManagerCommand::CompleteTask { task_id, peer_id })
            .await
            .map_err(|_| NetworkError::LocalShuttingDown)
    }

    /// Return the list of in-flight task ids.
    pub async fn active_tasks(&self) -> NetworkResult<Vec<TaskId>> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(TaskManagerCommand::ActiveTasks { reply: reply_tx })
            .await
            .map_err(|_| NetworkError::LocalShuttingDown)?;
        reply_rx.await.map_err(|_| NetworkError::LocalShuttingDown)
    }

    /// Request graceful shutdown.
    pub async fn shutdown(&self) -> NetworkResult<()> {
        self.cmd_tx
            .send(TaskManagerCommand::Shutdown)
            .await
            .map_err(|_| NetworkError::LocalShuttingDown)
    }
}

/// Reth-style task manager service.
///
/// Constructed via [`TaskManagerService::new`], which returns the
/// `(service, handle)` pair. The service is moved into a
/// `tokio::spawn`'d task that calls [`TaskManagerService::run`].
pub struct TaskManagerService {
    /// Per-peer command channel receiver.
    cmd_rx: mpsc::Receiver<TaskManagerCommand>,
    /// Event broadcast sender.
    event_tx: broadcast::Sender<NetworkEvent>,
    /// In-flight tasks, keyed by id.
    active: HashMap<TaskId, SyncTask>,
    /// Cancellation token used to break the timer tick on
    /// shutdown.
    shutdown: CancellationToken,
}

impl fmt::Debug for TaskManagerService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TaskManagerService")
            .field("active_tasks", &self.active.len())
            .field("cmd_capacity", &self.cmd_rx.capacity())
            .field("event_receivers", &self.event_tx.receiver_count())
            .finish()
    }
}

impl TaskManagerService {
    /// Build a fresh `(service, handle)` pair.
    pub fn new() -> (Self, TaskManagerHandle) {
        let (cmd_tx, cmd_rx) = mpsc::channel(256);
        let (event_tx, _event_rx) = broadcast::channel(64);
        let handle = TaskManagerHandle {
            cmd_tx,
            event_tx: event_tx.clone(),
        };
        let service = Self {
            cmd_rx,
            event_tx,
            active: HashMap::new(),
            shutdown: CancellationToken::new(),
        };
        (service, handle)
    }

    /// Drive the service loop until the command channel is closed
    /// or a `Shutdown` command is received.
    pub async fn run(mut self) {
        info!(target: "neo_network", "task manager service run loop started");
        while let Some(cmd) = self.cmd_rx.recv().await {
            match cmd {
                TaskManagerCommand::AddTask { task, reply } => {
                    let result = self.handle_add_task(task).await;
                    let _ = reply.send(result);
                }
                TaskManagerCommand::CancelTask { task_id } => {
                    self.handle_cancel_task(task_id).await;
                }
                TaskManagerCommand::CompleteTask { task_id, peer_id } => {
                    self.handle_complete_task(task_id, peer_id).await;
                }
                TaskManagerCommand::ActiveTasks { reply } => {
                    let ids: Vec<TaskId> = self.active.keys().copied().collect();
                    let _ = reply.send(ids);
                }
                TaskManagerCommand::Shutdown => {
                    info!(target: "neo_network", "task manager service shutdown requested");
                    self.shutdown.cancel();
                    break;
                }
            }
        }
        info!(target: "neo_network", "task manager service run loop exited");
    }

    // -----------------------------------------------------------------
    // Handlers
    // -----------------------------------------------------------------

    async fn handle_add_task(&mut self, task: SyncTask) -> NetworkResult<TaskId> {
        let task_id = TaskId::new();
        debug!(target: "neo_network", %task_id, "added sync task");
        self.active.insert(task_id, task);
        Ok(task_id)
    }

    async fn handle_cancel_task(&mut self, task_id: TaskId) {
        if self.active.remove(&task_id).is_some() {
            debug!(target: "neo_network", %task_id, "cancelled sync task");
        } else {
            warn!(target: "neo_network", %task_id, "cancel called for unknown task");
        }
    }

    async fn handle_complete_task(&mut self, task_id: TaskId, peer_id: PeerId) {
        if self.active.remove(&task_id).is_some() {
            debug!(
                target: "neo_network",
                %task_id,
                %peer_id,
                "completed sync task"
            );
        }
    }
}
