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
use crate::mpt_store::PreparedMptCommit;
use crate::state_store::ProjectedMptBlock;
use crate::state_store::StateStore;
use neo_crypto::mpt_trie::MptResult;
use neo_payloads::ApplicationExecuted;
use neo_payloads::Block;
use neo_payloads::{CommittedHandler, CommittingHandler};
use neo_storage::persistence::Store;
use neo_storage::persistence::providers::memory_store::MemoryStore;
use neo_storage::{DataCache, StorageResult};
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tracing::{debug, warn};

const DEFAULT_ASYNC_QUEUE_CAPACITY: usize = 256;
const DEFAULT_ASYNC_APPLY_BATCH_BLOCKS: usize = 8;
// Continuous catch-up should build a useful durable transaction before the
// worker races ahead of its producer. An idle producer still flushes a partial
// batch after ASYNC_BATCH_COALESCE_WAIT.
const ASYNC_EAGER_APPLY_BATCH_BLOCKS: usize = 2048;
// A block-only ceiling lets transaction-dense ranges create disproportionately
// large MDBX transactions. Bound projected mutations as a second work unit;
// one individually oversized block is still applied on its own.
/// Upper bound on projected storage changes drained into one async MPT apply.
///
/// Dense MainNet windows emit ~20k changes / 10k blocks. 12,288 keeps MDBX
/// transactions work-bounded (the 8,192 cap proved better than uncapped) while
/// reducing commit count versus 8,192 on the measured dense window.
const ASYNC_MAX_APPLY_BATCH_CHANGES: usize = 12_288;
const ASYNC_BATCH_COALESCE_WAIT: Duration = Duration::from_millis(10);

/// Handlers for wiring state-root MPT persistence into block persistence.
pub struct StateServiceCommitHandlers<S: Store = MemoryStore> {
    state_store: Arc<StateStore<S>>,
    worker: Option<AsyncStateRootWorker>,
    coordinated_requests: Option<parking_lot::Mutex<Vec<AsyncApplyRequest>>>,
    coordinated_projected_changes: Option<Arc<AtomicUsize>>,
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
            coordinated_requests: None,
            coordinated_projected_changes: None,
        }
    }

    /// Constructs a handler whose MPT bytes publish with the canonical store.
    ///
    /// This mode queues projected changes until
    /// [`Self::commit_pending_coordinated`] supplies the external atomic
    /// transaction. It cannot be combined with the ordinary async worker,
    /// because that worker intentionally publishes its backing store before the
    /// canonical Ledger fence.
    pub fn try_new_coordinated(state_store: Arc<StateStore<S>>) -> Result<Self, &'static str> {
        let Some(mpt) = state_store.mpt() else {
            return Err("coordinated StateService commits require an MPT store");
        };
        if !mpt.has_backing_store() {
            return Err("coordinated StateService commits require a durable MPT backing store");
        }
        Ok(Self {
            state_store,
            worker: None,
            coordinated_requests: Some(parking_lot::Mutex::new(Vec::new())),
            coordinated_projected_changes: Some(Arc::new(AtomicUsize::new(0))),
        })
    }

    /// Constructs a pipeline that projects MPT changes synchronously and applies
    /// them on a single background worker.
    ///
    /// The worker preserves block order and applies backpressure once the queue
    /// is full, so a sync burst can overlap native persistence with MPT writes
    /// without allowing unbounded memory growth.
    pub fn new_async(state_store: Arc<StateStore<S>>) -> Self {
        Self::new_async_with_limits(
            state_store,
            DEFAULT_ASYNC_QUEUE_CAPACITY,
            DEFAULT_ASYNC_APPLY_BATCH_BLOCKS,
        )
    }

    /// Constructs an async pipeline, returning the worker spawn failure instead
    /// of falling back to synchronous mode.
    pub fn try_new_async(state_store: Arc<StateStore<S>>) -> io::Result<Self> {
        Self::try_new_async_with_limits(
            state_store,
            DEFAULT_ASYNC_QUEUE_CAPACITY,
            DEFAULT_ASYNC_APPLY_BATCH_BLOCKS,
        )
    }

    /// Constructs an async pipeline with an explicit queue capacity and the
    /// default pipelined MPT apply limit.
    pub fn new_async_with_capacity(state_store: Arc<StateStore<S>>, queue_capacity: usize) -> Self {
        Self::new_async_with_limits(
            state_store,
            queue_capacity,
            DEFAULT_ASYNC_APPLY_BATCH_BLOCKS,
        )
    }

    /// Constructs an async pipeline with independent backpressure and MPT
    /// apply limits.
    ///
    /// The apply limit is a backlog ceiling. The worker flushes a smaller
    /// eager batch when it catches the producer, preserving pipeline overlap,
    /// and consumes up to this limit when queued work is already available.
    /// Queue capacity remains independent and large enough to absorb bursts.
    pub fn new_async_with_limits(
        state_store: Arc<StateStore<S>>,
        queue_capacity: usize,
        max_apply_batch_blocks: usize,
    ) -> Self {
        match Self::try_new_async_with_limits(
            Arc::clone(&state_store),
            queue_capacity,
            max_apply_batch_blocks,
        ) {
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
        Self::try_new_async_with_limits(
            state_store,
            queue_capacity,
            DEFAULT_ASYNC_APPLY_BATCH_BLOCKS,
        )
    }

    /// Constructs an async pipeline with independent queue and apply limits,
    /// returning worker spawn failures to the caller.
    pub fn try_new_async_with_limits(
        state_store: Arc<StateStore<S>>,
        queue_capacity: usize,
        max_apply_batch_blocks: usize,
    ) -> io::Result<Self> {
        let worker = AsyncStateRootWorker::spawn(
            Arc::clone(&state_store),
            queue_capacity,
            max_apply_batch_blocks,
        )?;
        Ok(Self {
            state_store,
            worker: Some(worker),
            coordinated_requests: None,
            coordinated_projected_changes: None,
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

    /// Returns whether StateService publication is delegated to the canonical
    /// transaction coordinator.
    pub fn is_coordinated(&self) -> bool {
        self.coordinated_requests.is_some()
    }

    /// Total projected storage changes queued for the next coordinated commit.
    pub fn pending_coordinated_projected_changes(&self) -> usize {
        self.coordinated_projected_changes
            .as_ref()
            .map_or(0, |changes| changes.load(Ordering::Acquire))
    }

    /// Returns the async MPT worker queue capacity when async mode is enabled.
    pub fn async_queue_capacity(&self) -> Option<usize> {
        self.worker
            .as_ref()
            .map(AsyncStateRootWorker::queue_capacity)
    }

    /// Maximum consecutive roots applied in one worker trie batch.
    pub fn async_apply_batch_limit(&self) -> Option<usize> {
        self.worker
            .as_ref()
            .map(AsyncStateRootWorker::max_apply_batch_blocks)
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
        if self.is_coordinated() {
            return Err(
                "coordinated StateService mode requires commit_pending_coordinated".to_string(),
            );
        }
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
        if self.coordinated_requests.is_some() {
            return self.on_committing_coordinated(block_index, snapshot);
        }
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
        if self.coordinated_requests.is_some() {
            return self.on_committing_coordinated(block_index, snapshot);
        }
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

    fn on_committing_coordinated<B: neo_storage::CacheRead>(
        &self,
        block_index: u32,
        snapshot: &DataCache<B>,
    ) -> bool {
        let Some(requests) = &self.coordinated_requests else {
            return false;
        };
        let total_start = std::time::Instant::now();
        let project_start = std::time::Instant::now();
        let mut changes = Vec::with_capacity(snapshot.pending_change_count());
        StateStore::<MemoryStore>::project_mpt_changes_into(snapshot, &mut changes);
        let projected_change_count = changes.len();
        let mut requests = requests.lock();
        requests.push(AsyncApplyRequest {
            block_index,
            changes,
            project_us: elapsed_us(project_start),
            queued_at: std::time::Instant::now(),
            total_start,
        });
        if let Some(total) = &self.coordinated_projected_changes {
            total.fetch_add(projected_change_count, Ordering::Release);
        }
        true
    }

    /// Applies all queued projected blocks and commits their prepared MPT bytes
    /// through `commit`.
    ///
    /// `Ok(None)` means no StateService block was queued and the caller should
    /// use its ordinary canonical commit. `Ok(Some(roots))` means `commit` was
    /// invoked exactly once and the StateService local generation now reflects
    /// those roots. The trusted callback must consume the complete prepared
    /// overlay and return success only after it has atomically published the
    /// canonical and StateService bytes. An error leaves the in-memory
    /// StateService root at its previously committed value, although recovery
    /// of any external side effects remains the coordinator's responsibility.
    pub fn commit_pending_coordinated<F>(
        &self,
        commit: F,
    ) -> Result<Option<Vec<neo_primitives::UInt256>>, String>
    where
        F: FnOnce(&S, &mut PreparedMptCommit) -> StorageResult<()>,
    {
        let Some(requests) = &self.coordinated_requests else {
            return Err("StateService handler is not in coordinated commit mode".to_string());
        };
        let batch = {
            let mut requests = requests.lock();
            let batch = std::mem::take(&mut *requests);
            if let Some(total) = &self.coordinated_projected_changes {
                total.store(0, Ordering::Release);
            }
            batch
        };
        if batch.is_empty() {
            return Ok(None);
        }

        StateRootApplyMetrics::record_count(
            StateRootApplyCountKind::BatchBlocks,
            batch.len() as u64,
        );
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
        match self
            .state_store
            .apply_projected_mpt_change_batch_coordinated(&projected, commit)
        {
            Ok(roots) => {
                for (request, root_hash) in batch.iter().zip(roots.iter()) {
                    let _ = log_applied_root(request.block_index, root_hash);
                }
                Ok(Some(roots))
            }
            Err(error) => {
                if let Some(request) = batch.first() {
                    let _ = log_failed_root(request.block_index, &error);
                }
                Err(error.to_string())
            }
        }
    }

    /// Drops projected blocks that never crossed the canonical transaction.
    pub fn discard_pending_coordinated(&self) {
        if let Some(requests) = &self.coordinated_requests {
            requests.lock().clear();
        }
        if let Some(total) = &self.coordinated_projected_changes {
            total.store(0, Ordering::Release);
        }
    }

    /// Discards any state-root candidate whose block index falls in
    /// the supplied range (inclusive).
    pub fn on_reverting(&self, from_index: u32, to_index: u32) -> bool {
        self.discard_pending_coordinated();
        if self.is_coordinated() {
            warn!(
                target: "neo.state_service",
                from_index,
                to_index,
                "coordinated StateService revert requires on_reverting_coordinated"
            );
            return false;
        }
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
                return false;
            }
            return true;
        }
        self.apply_reverting(from_index, to_index)
    }

    /// Applies a revert through the same external coordinator used by forward
    /// split-store publication.
    pub fn on_reverting_coordinated<F>(
        &self,
        from_index: u32,
        to_index: u32,
        commit: F,
    ) -> Result<(), String>
    where
        F: FnOnce(&S, &mut PreparedMptCommit) -> StorageResult<()>,
    {
        if !self.is_coordinated() {
            return Err("StateService handler is not in coordinated commit mode".to_string());
        }
        self.discard_pending_coordinated();
        discard_state_roots(&self.state_store, from_index, to_index);
        let Some(mpt) = self.state_store.mpt() else {
            return Err("coordinated StateService handler has no MPT store".to_string());
        };
        mpt.revert_local_roots_coordinated(from_index, to_index, commit)
            .map_err(|error| error.to_string())
    }

    fn apply_reverting(&self, from_index: u32, to_index: u32) -> bool {
        if let Err(err) = apply_reverting(&self.state_store, from_index, to_index) {
            warn!(
                target: "neo.state_service",
                from_index,
                to_index,
                %err,
                "local state-root revert failed"
            );
            return false;
        }
        true
    }
}

fn discard_state_roots<S>(state_store: &StateStore<S>, from_index: u32, to_index: u32)
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
}

fn apply_reverting<S>(state_store: &StateStore<S>, from_index: u32, to_index: u32) -> MptResult<()>
where
    S: Store,
{
    discard_state_roots(state_store, from_index, to_index);
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
    max_apply_batch_blocks: usize,
    failed: Arc<AtomicBool>,
    recycled_change_buffers: Arc<parking_lot::Mutex<Vec<Vec<crate::mpt_store::MptChange>>>>,
    #[cfg(test)]
    applied_batch_sizes: Arc<parking_lot::Mutex<Vec<usize>>>,
    handle: parking_lot::Mutex<Option<JoinHandle<()>>>,
}

impl AsyncStateRootWorker {
    fn spawn<S>(
        state_store: Arc<StateStore<S>>,
        queue_capacity: usize,
        max_apply_batch_blocks: usize,
    ) -> io::Result<Self>
    where
        S: Store + 'static,
    {
        let capacity = queue_capacity.max(1);
        let max_apply_batch_blocks = max_apply_batch_blocks.max(1).min(capacity);
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
                    max_apply_batch_blocks,
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
            max_apply_batch_blocks,
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

    fn max_apply_batch_blocks(&self) -> usize {
        self.max_apply_batch_blocks
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
                collect_apply_batch(
                    &rx,
                    &mut pending_command,
                    &mut batch,
                    max_batch_blocks,
                    ASYNC_MAX_APPLY_BATCH_CHANGES,
                );
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
    max_batch_changes: usize,
) {
    let eager_batch_blocks = ASYNC_EAGER_APPLY_BATCH_BLOCKS.min(max_batch_blocks.max(1));
    let max_batch_changes = max_batch_changes.max(1);
    let mut batch_changes = batch.iter().fold(0usize, |total, request| {
        total.saturating_add(request.changes.len())
    });
    while batch.len() < max_batch_blocks.max(1) {
        match rx.try_recv() {
            Ok(AsyncCommand::Apply(next)) => {
                if let Err(next) =
                    try_push_apply_request(batch, &mut batch_changes, next, max_batch_changes)
                {
                    *pending_command = Some(AsyncCommand::Apply(next));
                    return;
                }
            }
            Ok(other) => {
                *pending_command = Some(other);
                return;
            }
            Err(TryRecvError::Disconnected) => return,
            Err(TryRecvError::Empty) => {
                if batch.len() >= eager_batch_blocks {
                    return;
                }
                match rx.recv_timeout(ASYNC_BATCH_COALESCE_WAIT) {
                    Ok(AsyncCommand::Apply(next)) => {
                        if let Err(next) = try_push_apply_request(
                            batch,
                            &mut batch_changes,
                            next,
                            max_batch_changes,
                        ) {
                            *pending_command = Some(AsyncCommand::Apply(next));
                            return;
                        }
                    }
                    Ok(other) => {
                        *pending_command = Some(other);
                        return;
                    }
                    Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return,
                }
            }
        }
    }
}

fn try_push_apply_request(
    batch: &mut Vec<AsyncApplyRequest>,
    batch_changes: &mut usize,
    request: AsyncApplyRequest,
    max_batch_changes: usize,
) -> Result<(), AsyncApplyRequest> {
    let next_changes = request.changes.len();
    if !batch.is_empty() && batch_changes.saturating_add(next_changes) > max_batch_changes.max(1) {
        return Err(request);
    }
    *batch_changes = batch_changes.saturating_add(next_changes);
    batch.push(request);
    Ok(())
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
