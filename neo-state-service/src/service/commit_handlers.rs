//! Block-commit handler pipeline for the state service.
//!
//! Wires local MPT state-root persistence into the block persistence
//! pipeline:
//!
//! - On `Committing(block, snapshot, ...)` - projects the snapshot's
//!   tracked storage changes into the persisted MPT via
//!   [`StateStore::apply_snapshot_changes`].
//! - On explicit revert handling - drops any candidate state roots whose
//!   block index falls in the reverting range via [`StateStore::discard`].
//!
//! The handler is intentionally a thin adapter over [`StateStore`], so the
//! C# `Blockchain_Committing_Handler` filtering rules live in one place.

use crate::StateRootApplyMetrics;
use crate::metrics::{StateRootApplyCountKind, StateRootApplyStage};
use crate::state_store::ProjectedMptBlock;
use crate::state_store::StateStore;
use neo_crypto::mpt_trie::MptResult;
use neo_payloads::ApplicationExecuted;
use neo_payloads::Block;
use neo_payloads::{CommittedHandler, CommittingHandler};
use neo_storage::DataCache;
use neo_storage::persistence::Store;
use neo_storage::persistence::providers::memory_store::MemoryStore;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tracing::{debug, warn};

const DEFAULT_ASYNC_QUEUE_CAPACITY: usize = 256;
const ASYNC_BATCH_COALESCE_WAIT: Duration = Duration::from_millis(10);

/// Handlers for wiring state-root MPT persistence into block persistence.
pub struct StateServiceCommitHandlers<S: Store = MemoryStore> {
    state_store: Arc<StateStore<S>>,
    worker: Option<AsyncStateRootWorker>,
}

impl<S> StateServiceCommitHandlers<S>
where
    S: Store + 'static,
{
    /// Constructs a new pipeline backed by the supplied state store.
    pub fn new(state_store: Arc<StateStore<S>>) -> Self {
        Self {
            state_store,
            worker: None,
        }
    }

    /// Constructs a pipeline that projects MPT changes synchronously and applies
    /// them on a single background worker.
    ///
    /// The worker preserves block order and applies backpressure once the queue
    /// is full, so a sync burst can overlap native persistence with MPT writes
    /// without allowing unbounded memory growth.
    pub fn new_async(state_store: Arc<StateStore<S>>) -> Self {
        Self::new_async_with_capacity(state_store, DEFAULT_ASYNC_QUEUE_CAPACITY)
    }

    /// Constructs an async pipeline, returning the worker spawn failure instead
    /// of falling back to synchronous mode.
    pub fn try_new_async(state_store: Arc<StateStore<S>>) -> io::Result<Self> {
        Self::try_new_async_with_capacity(state_store, DEFAULT_ASYNC_QUEUE_CAPACITY)
    }

    /// Constructs an async pipeline with an explicit queue capacity.
    pub fn new_async_with_capacity(state_store: Arc<StateStore<S>>, queue_capacity: usize) -> Self {
        match Self::try_new_async_with_capacity(Arc::clone(&state_store), queue_capacity) {
            Ok(handlers) => handlers,
            Err(err) => {
                warn!(
                    target: "neo.state_service",
                    error = %err,
                    "failed to spawn local state-root worker; falling back to synchronous commits"
                );
                Self::new(state_store)
            }
        }
    }

    /// Constructs an async pipeline with an explicit queue capacity, returning
    /// the worker spawn failure instead of falling back to synchronous mode.
    pub fn try_new_async_with_capacity(
        state_store: Arc<StateStore<S>>,
        queue_capacity: usize,
    ) -> io::Result<Self> {
        let worker = AsyncStateRootWorker::spawn(Arc::clone(&state_store), queue_capacity)?;
        Ok(Self {
            state_store,
            worker: Some(worker),
        })
    }

    /// Returns a clone of the inner state store.
    pub fn state_store(&self) -> Arc<StateStore<S>> {
        Arc::clone(&self.state_store)
    }

    /// Returns true when MPT applies are serialized on the background worker.
    pub fn is_async(&self) -> bool {
        self.worker.is_some()
    }

    /// Returns the async MPT worker queue capacity when async mode is enabled.
    pub fn async_queue_capacity(&self) -> Option<usize> {
        self.worker
            .as_ref()
            .map(AsyncStateRootWorker::queue_capacity)
    }

    /// Number of reusable async projection buffers currently parked.
    #[cfg(test)]
    pub(crate) fn recycled_change_buffer_count(&self) -> usize {
        self.worker
            .as_ref()
            .map_or(0, AsyncStateRootWorker::recycled_change_buffer_count)
    }

    /// Worker batch sizes observed by the async MPT apply path.
    #[cfg(test)]
    pub(crate) fn applied_batch_sizes(&self) -> Vec<usize> {
        self.worker
            .as_ref()
            .map_or_else(Vec::new, AsyncStateRootWorker::applied_batch_sizes)
    }

    /// Blocks until all queued async MPT work has completed.
    ///
    /// Synchronous handlers return immediately. Async handlers return `false`
    /// if the worker has already observed an MPT apply failure.
    pub fn flush(&self) -> bool {
        self.flush_result().is_ok()
    }

    /// Blocks until all queued async MPT work has completed, returning the
    /// observed worker failure to callers that must fail hard on shutdown.
    pub fn flush_result(&self) -> Result<(), &'static str> {
        self.worker
            .as_ref()
            .map_or(Ok(()), AsyncStateRootWorker::flush_result)
    }

    /// Drains queued MPT work and fences the backing store before the
    /// canonical Ledger transaction is allowed to commit.
    pub fn flush_durable_result(&self) -> Result<(), String> {
        self.flush_result().map_err(str::to_string)?;
        if let Some(mpt) = self.state_store.mpt() {
            mpt.flush_backing().map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    /// Applies the block snapshot's storage changes to the local MPT state
    /// root store.
    pub fn on_committing<B: neo_storage::CacheRead>(
        &self,
        block_index: u32,
        snapshot: &DataCache<B>,
    ) -> bool {
        if let Some(worker) = &self.worker {
            return self.on_committing_async(worker, block_index, snapshot)
                && worker.flush_result().is_ok();
        }
        self.apply_committing(block_index, snapshot)
    }

    /// Queues the block snapshot's storage changes for trusted bulk-sync
    /// callers that will fence the worker at the batch boundary.
    pub fn on_committing_deferred<B: neo_storage::CacheRead>(
        &self,
        block_index: u32,
        snapshot: &DataCache<B>,
    ) -> bool {
        if let Some(worker) = &self.worker {
            return self.on_committing_async(worker, block_index, snapshot);
        }
        self.apply_committing(block_index, snapshot)
    }

    fn apply_committing<B: neo_storage::CacheRead>(
        &self,
        block_index: u32,
        snapshot: &DataCache<B>,
    ) -> bool {
        match self
            .state_store
            .apply_snapshot_changes(block_index, snapshot)
        {
            Ok(Some(root_hash)) => log_applied_root(block_index, &root_hash),
            Ok(None) => log_skipped_root(block_index),
            Err(err) => log_failed_root(block_index, &err),
        }
    }

    fn on_committing_async<B: neo_storage::CacheRead>(
        &self,
        worker: &AsyncStateRootWorker,
        block_index: u32,
        snapshot: &DataCache<B>,
    ) -> bool {
        if !worker.is_healthy() {
            warn!(
                target: "neo.state_service",
                block_index,
                "local state-root worker has failed"
            );
            return false;
        }
        let total_start = std::time::Instant::now();
        let project_start = std::time::Instant::now();
        let mut changes = worker.take_change_buffer(snapshot.pending_change_count());
        StateStore::<neo_storage::persistence::providers::memory_store::MemoryStore>::project_mpt_changes_into(snapshot, &mut changes);
        let project_us = elapsed_us(project_start);
        worker.enqueue(AsyncApplyRequest {
            block_index,
            changes,
            project_us,
            queued_at: std::time::Instant::now(),
            total_start,
        })
    }

    /// Discards any state-root candidate whose block index falls in
    /// the supplied range (inclusive).
    pub fn on_reverting(&self, from_index: u32, to_index: u32) {
        if let Some(worker) = &self.worker {
            if let Err(err) = worker.enqueue_command(AsyncCommand::Revert {
                from_index,
                to_index,
            }) {
                warn!(
                    target: "neo.state_service",
                    from_index,
                    to_index,
                    error = err,
                    "failed to enqueue local state-root revert"
                );
            }
            return;
        }
        self.apply_reverting(from_index, to_index);
    }

    fn apply_reverting(&self, from_index: u32, to_index: u32) {
        if let Err(err) = apply_reverting(&self.state_store, from_index, to_index) {
            warn!(
                target: "neo.state_service",
                from_index,
                to_index,
                %err,
                "local state-root revert failed"
            );
        }
    }
}

fn apply_reverting<S>(state_store: &StateStore<S>, from_index: u32, to_index: u32) -> MptResult<()>
where
    S: Store,
{
    for index in from_index..=to_index {
        if let Some(root) =
            state_store.get_state_root(crate::state_store::StateStoreLookup::ByBlockIndex(index))
        {
            state_store.discard(root.root_hash());
        }
    }
    if let Some(mpt) = state_store.mpt() {
        mpt.revert_local_roots(from_index, to_index)?;
    }
    Ok(())
}

fn log_applied_root(block_index: u32, root_hash: &neo_primitives::UInt256) -> bool {
    debug!(
        target: "neo.state_service",
        block_index,
        %root_hash,
        "applied local state root"
    );
    true
}

fn log_skipped_root(block_index: u32) -> bool {
    debug!(
        target: "neo.state_service",
        block_index,
        "state service has no MPT backend; skipping local state-root update"
    );
    true
}

fn log_failed_root(block_index: u32, err: &neo_crypto::mpt_trie::MptError) -> bool {
    warn!(
        target: "neo.state_service",
        block_index,
        %err,
        "local state-root update failed"
    );
    false
}

fn elapsed_us(start: std::time::Instant) -> u64 {
    start.elapsed().as_micros().min(u64::MAX as u128) as u64
}

pub(crate) struct AsyncApplyRequest {
    block_index: u32,
    changes: Vec<crate::mpt_store::MptChange>,
    project_us: u64,
    queued_at: std::time::Instant,
    total_start: std::time::Instant,
}

enum AsyncCommand {
    Apply(AsyncApplyRequest),
    Revert { from_index: u32, to_index: u32 },
    Flush(SyncSender<bool>),
    Stop,
}

struct AsyncStateRootWorker {
    tx: SyncSender<AsyncCommand>,
    queue_capacity: usize,
    failed: Arc<AtomicBool>,
    recycled_change_buffers: Arc<parking_lot::Mutex<Vec<Vec<crate::mpt_store::MptChange>>>>,
    #[cfg(test)]
    applied_batch_sizes: Arc<parking_lot::Mutex<Vec<usize>>>,
    handle: parking_lot::Mutex<Option<JoinHandle<()>>>,
}

impl AsyncStateRootWorker {
    fn spawn<S>(state_store: Arc<StateStore<S>>, queue_capacity: usize) -> io::Result<Self>
    where
        S: Store + 'static,
    {
        let capacity = queue_capacity.max(1);
        let (tx, rx) = std::sync::mpsc::sync_channel(capacity);
        let failed = Arc::new(AtomicBool::new(false));
        let worker_failed = Arc::clone(&failed);
        let recycled_change_buffers = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let worker_recycled_change_buffers = Arc::clone(&recycled_change_buffers);
        #[cfg(test)]
        let applied_batch_sizes = Arc::new(parking_lot::Mutex::new(Vec::new()));
        #[cfg(test)]
        let worker_applied_batch_sizes = Arc::clone(&applied_batch_sizes);
        let handle = thread::Builder::new()
            .name("neo-state-root-mpt".to_string())
            .spawn(move || {
                worker_loop(
                    state_store,
                    rx,
                    capacity,
                    worker_failed,
                    worker_recycled_change_buffers,
                    #[cfg(test)]
                    worker_applied_batch_sizes,
                )
            })
            .map_err(|err| {
                warn!(
                    target: "neo.state_service",
                    error = %err,
                    "failed to spawn local state-root worker"
                );
                err
            })?;

        Ok(Self {
            tx,
            queue_capacity: capacity,
            failed,
            recycled_change_buffers,
            #[cfg(test)]
            applied_batch_sizes,
            handle: parking_lot::Mutex::new(Some(handle)),
        })
    }

    fn queue_capacity(&self) -> usize {
        self.queue_capacity
    }

    fn is_healthy(&self) -> bool {
        !self.failed.load(Ordering::Acquire)
    }

    fn take_change_buffer(&self, capacity_hint: usize) -> Vec<crate::mpt_store::MptChange> {
        let mut buffer = self
            .recycled_change_buffers
            .lock()
            .pop()
            .unwrap_or_default();
        buffer.clear();
        buffer.reserve(capacity_hint);
        buffer
    }

    #[cfg(test)]
    fn recycled_change_buffer_count(&self) -> usize {
        self.recycled_change_buffers.lock().len()
    }

    #[cfg(test)]
    fn applied_batch_sizes(&self) -> Vec<usize> {
        self.applied_batch_sizes.lock().clone()
    }

    fn enqueue(&self, request: AsyncApplyRequest) -> bool {
        let enqueue_start = std::time::Instant::now();
        let result = self.enqueue_command(AsyncCommand::Apply(request));
        StateRootApplyMetrics::record_stage(
            StateRootApplyStage::EnqueueBlocking,
            elapsed_us(enqueue_start),
        );
        result.is_ok()
    }

    fn enqueue_command(&self, command: AsyncCommand) -> Result<(), &'static str> {
        if !self.is_healthy() {
            return Err("state-root worker has already failed");
        }
        match self.tx.send(command) {
            Ok(()) => Ok(()),
            Err(err) => {
                self.failed.store(true, Ordering::Release);
                warn!(
                    target: "neo.state_service",
                    error = %err,
                    "failed to enqueue local state-root worker command"
                );
                Err("state-root worker command channel is closed")
            }
        }
    }

    fn flush_result(&self) -> Result<(), &'static str> {
        if !self.is_healthy() {
            return Err("state-root worker has already failed");
        }
        let (reply_tx, reply_rx) = std::sync::mpsc::sync_channel(0);
        if self.tx.send(AsyncCommand::Flush(reply_tx)).is_err() {
            self.failed.store(true, Ordering::Release);
            return Err("state-root worker command channel is closed");
        }
        match reply_rx.recv() {
            Ok(true) if self.is_healthy() => Ok(()),
            Ok(_) => Err("state-root worker reported a failed operation"),
            Err(_) => {
                self.failed.store(true, Ordering::Release);
                Err("state-root worker stopped before flush completed")
            }
        }
    }
}

impl Drop for AsyncStateRootWorker {
    fn drop(&mut self) {
        if let Err(err) = self.flush_result() {
            warn!(
                target: "neo.state_service",
                error = err,
                "StateService MPT worker flush failed during drop"
            );
        }
        let _ = self.tx.send(AsyncCommand::Stop);
        if let Some(handle) = self.handle.lock().take() {
            if handle.join().is_err() {
                warn!(
                    target: "neo.state_service",
                    "StateService MPT worker panicked during shutdown"
                );
            }
        }
    }
}

fn worker_loop<S>(
    state_store: Arc<StateStore<S>>,
    rx: Receiver<AsyncCommand>,
    max_batch_blocks: usize,
    failed: Arc<AtomicBool>,
    recycled_change_buffers: Arc<parking_lot::Mutex<Vec<Vec<crate::mpt_store::MptChange>>>>,
    #[cfg(test)] applied_batch_sizes: Arc<parking_lot::Mutex<Vec<usize>>>,
) where
    S: Store,
{
    let mut pending_command = None;
    loop {
        let command = match pending_command.take() {
            Some(command) => command,
            None => match rx.recv() {
                Ok(command) => command,
                Err(_) => break,
            },
        };
        match command {
            AsyncCommand::Apply(request) => {
                let mut batch = vec![request];
                collect_apply_batch(&rx, &mut pending_command, &mut batch, max_batch_blocks);
                apply_request_batch(
                    &state_store,
                    batch,
                    &failed,
                    &recycled_change_buffers,
                    #[cfg(test)]
                    &applied_batch_sizes,
                );
            }
            AsyncCommand::Revert {
                from_index,
                to_index,
            } => {
                if let Err(err) = apply_reverting(&state_store, from_index, to_index) {
                    failed.store(true, Ordering::Release);
                    warn!(
                        target: "neo.state_service",
                        from_index,
                        to_index,
                        %err,
                        "local state-root revert failed"
                    );
                }
            }
            AsyncCommand::Flush(reply) => {
                let _ = reply.send(!failed.load(Ordering::Acquire));
            }
            AsyncCommand::Stop => break,
        }
    }
}

fn collect_apply_batch(
    rx: &Receiver<AsyncCommand>,
    pending_command: &mut Option<AsyncCommand>,
    batch: &mut Vec<AsyncApplyRequest>,
    max_batch_blocks: usize,
) {
    while batch.len() < max_batch_blocks.max(1) {
        match rx.try_recv() {
            Ok(AsyncCommand::Apply(next)) => {
                batch.push(next);
            }
            Ok(other) => {
                *pending_command = Some(other);
                return;
            }
            Err(TryRecvError::Disconnected) => return,
            Err(TryRecvError::Empty) => match rx.recv_timeout(ASYNC_BATCH_COALESCE_WAIT) {
                Ok(AsyncCommand::Apply(next)) => batch.push(next),
                Ok(other) => {
                    *pending_command = Some(other);
                    return;
                }
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return,
            },
        }
    }
}

fn apply_request_batch<S>(
    state_store: &StateStore<S>,
    mut batch: Vec<AsyncApplyRequest>,
    failed: &AtomicBool,
    recycled_change_buffers: &parking_lot::Mutex<Vec<Vec<crate::mpt_store::MptChange>>>,
    #[cfg(test)] applied_batch_sizes: &parking_lot::Mutex<Vec<usize>>,
) where
    S: Store,
{
    StateRootApplyMetrics::record_count(StateRootApplyCountKind::BatchBlocks, batch.len() as u64);
    #[cfg(test)]
    applied_batch_sizes.lock().push(batch.len());

    for request in &batch {
        StateRootApplyMetrics::record_stage(
            StateRootApplyStage::QueueWait,
            elapsed_us(request.queued_at),
        );
    }

    let projected = batch
        .iter()
        .map(|request| ProjectedMptBlock {
            block_index: request.block_index,
            changes: request.changes.as_slice(),
            project_us: request.project_us,
            total_start: request.total_start,
        })
        .collect::<Vec<_>>();
    match state_store.apply_projected_mpt_change_batch(&projected) {
        Ok(roots) => {
            if roots.is_empty() {
                for request in &batch {
                    let _ = log_skipped_root(request.block_index);
                }
            } else {
                for (request, root_hash) in batch.iter().zip(roots.iter()) {
                    let _ = log_applied_root(request.block_index, root_hash);
                }
            }
        }
        Err(err) => {
            failed.store(true, Ordering::Release);
            if let Some(request) = batch.first() {
                let _ = log_failed_root(request.block_index, &err);
            }
        }
    }

    let mut recycled = recycled_change_buffers.lock();
    for request in batch.drain(..) {
        let mut changes = request.changes;
        changes.clear();
        recycled.push(changes);
    }
}

impl<S> CommittedHandler for StateServiceCommitHandlers<S>
where
    S: Store + 'static,
{
    fn blockchain_committed_handler(&self, _network: u32, block: &Block) {
        debug!(
            target: "neo.state_service",
            block_index = block.index(),
            "state service committed handler observed block"
        );
    }
}

impl<S> CommittingHandler for StateServiceCommitHandlers<S>
where
    S: Store + 'static,
{
    fn blockchain_committing_handler<B: neo_storage::CacheRead>(
        &self,
        _network: u32,
        block: &Block,
        snapshot: &DataCache<B>,
        _application_executed_list: &[ApplicationExecuted],
    ) {
        let _ = self.on_committing(block.index(), snapshot);
    }
}

#[cfg(test)]
#[path = "../tests/service/commit_handlers.rs"]
mod tests;
