//! # Advisory signature preverification
//!
//! The pool deliberately owns only pure, state-independent ECDSA work. A
//! successful ticket carries an exact-input cache for the canonical NeoVM
//! witness verifier; it never authorizes a header or permits the caller to skip
//! canonical witness execution.
//!
//! ## Boundary
//!
//! Workers may hash and verify standard witness signatures without reading
//! chain state. Canonical parent checks, NeoVM execution, state publication,
//! and fallback policy remain owned by the ordered blockchain pipeline.
//!
//! ## Contents
//!
//! - Bounded worker-pool configuration and fixed-cardinality metrics.
//! - Exact-input header preverification tickets and cancellation.
//! - Ordered look-ahead windows for header, inventory, and archive import.
//! - Existing state-independent transaction verification receipts.

#[cfg(test)]
use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use neo_config::{Hardfork, ProtocolSettings};
use neo_crypto::Crypto;
use neo_execution::{
    PreverifiedSignatureCache, PreverifiedSignatureCacheMetricsSnapshot,
    preverify_standard_witness_signatures,
};
use neo_io::{BinaryWriter, Serializable};
use neo_payloads::{Header, Transaction, Witness};
use neo_primitives::UInt256;

mod window;

pub(crate) use window::OrderedHeaderVerificationWindow;

/// Configuration for the bounded header-witness verification pool.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SignatureVerificationPoolConfig {
    /// Number of verification workers.
    pub workers: usize,
    /// Number of jobs allowed to wait behind workers.
    pub queue_capacity: usize,
}

impl Default for SignatureVerificationPoolConfig {
    fn default() -> Self {
        Self {
            workers: 4,
            queue_capacity: 32,
        }
    }
}

impl SignatureVerificationPoolConfig {
    /// Maximum number of outstanding tickets retained by a caller.
    #[must_use]
    pub const fn window(self) -> usize {
        self.workers.saturating_add(self.queue_capacity)
    }

    /// Validates bounds before any worker thread is started.
    pub fn validate(self) -> Result<(), SignatureVerificationPoolConfigError> {
        if self.workers == 0 {
            return Err(SignatureVerificationPoolConfigError::ZeroWorkers);
        }
        if self.workers > 64 {
            return Err(SignatureVerificationPoolConfigError::TooManyWorkers {
                workers: self.workers,
            });
        }
        if self.queue_capacity == 0 {
            return Err(SignatureVerificationPoolConfigError::ZeroQueue);
        }
        if self.queue_capacity > 4096 {
            return Err(SignatureVerificationPoolConfigError::QueueTooLarge {
                capacity: self.queue_capacity,
            });
        }
        Ok(())
    }
}

/// Invalid pool configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SignatureVerificationPoolConfigError {
    /// At least one worker is required.
    #[error("signature verification pool requires at least one worker")]
    ZeroWorkers,
    /// A runaway worker count would contend with VM execution.
    #[error("signature verification pool worker count {workers} exceeds the hard limit 64")]
    TooManyWorkers {
        /// Requested worker count.
        workers: usize,
    },
    /// A zero queue would make the pool impossible to use.
    #[error("signature verification pool requires a non-zero queue capacity")]
    ZeroQueue,
    /// Bound queued memory and verification work.
    #[error("signature verification pool queue capacity {capacity} exceeds the hard limit 4096")]
    QueueTooLarge {
        /// Requested queue capacity.
        capacity: usize,
    },
    /// The operating system refused to create a worker thread.
    #[error("signature verification worker thread could not be started")]
    WorkerSpawnFailed,
}

/// Failure from a signature-verification job.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SignatureVerificationError {
    /// A state-independent transaction signature check failed.
    #[error("signature verification failed: {0}")]
    InvalidWitness(String),
    /// The owning speculative batch was invalidated before this queued job
    /// began preverification work.
    #[error("signature preverification was cancelled before execution")]
    Cancelled,
    /// A worker panicked while executing preverification.
    #[error("signature preverification worker panicked")]
    WorkerPanicked,
    /// The worker disappeared before returning a result.
    #[error("signature preverification worker became unavailable")]
    WorkerUnavailable,
}

/// Queue admission failure.  A full queue is an ordinary backpressure signal;
/// callers should wait for an older ticket and retry.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SignatureVerificationSubmitError {
    /// The bounded queue is full.
    #[error("signature verification queue is full")]
    QueueFull,
    /// The pool has been shut down.
    #[error("signature verification pool is closed")]
    Closed,
    /// Input hashing or encoding failed before queue admission.
    #[error("signature verification job could not be prepared: {0}")]
    InvalidInput(&'static str),
}

/// Batch-scoped cancellation shared by queued speculative jobs.
///
/// Dropping the owner invalidates jobs that have not started. A job already
/// executing preverification is allowed to finish.
pub(crate) struct SignatureVerificationCancellation {
    cancelled: Arc<AtomicBool>,
}

impl Default for SignatureVerificationCancellation {
    fn default() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl SignatureVerificationCancellation {
    fn token(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancelled)
    }

    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    #[cfg(test)]
    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

impl Drop for SignatureVerificationCancellation {
    fn drop(&mut self) {
        self.cancel();
    }
}

type Job = Box<dyn FnOnce() + Send + 'static>;

/// Cumulative counters for one signature-verification pool.
///
/// `submitted` counts jobs accepted by the bounded queue.  The result
/// counters are mutually exclusive for jobs that reached a worker; queue
/// admission failures are recorded separately.  Counters are intentionally
/// monotonic and scoped to the lifetime of the pool.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SignatureVerificationPoolMetricsSnapshot {
    /// Jobs accepted by the bounded queue.
    pub submitted: u64,
    /// Jobs that completed normally, including advisory fallback results.
    pub completed: u64,
    /// State-independent transaction jobs rejected by signature verification.
    pub invalid: u64,
    /// Queued jobs cancelled after their speculative batch was invalidated.
    pub cancelled: u64,
    /// Jobs terminated by a worker panic.
    pub worker_panics: u64,
    /// Tickets that observed a worker/result channel becoming unavailable.
    pub worker_unavailable: u64,
    /// Submission attempts rejected because the bounded queue was full.
    pub queue_full: u64,
    /// Submission attempts rejected because the pool was closed.
    pub queue_closed: u64,
    /// Standard header witnesses that produced an advisory signature cache.
    pub header_standard_caches_prepared: u64,
    /// Unsupported header witnesses left on the ordinary canonical verifier.
    pub header_unsupported_witness_fallbacks: u64,
    /// Exact header ECDSA operations computed and retained by advisory caches.
    pub header_preverified_ecdsa_operations: u64,
    /// Header caches installed on a canonical NeoVM application engine.
    pub header_canonical_cache_consumptions: u64,
    /// Exact signature-cache lookups made by canonical header verification.
    pub header_canonical_cache_lookups: u64,
    /// Canonical header lookups that reused a preverified outcome.
    pub header_canonical_cache_hits: u64,
    /// Canonical header lookups that fell back to ordinary curve verification.
    pub header_canonical_cache_misses: u64,
}

impl SignatureVerificationPoolMetricsSnapshot {
    /// Number of jobs accepted by the queue.
    #[must_use]
    pub const fn accepted(self) -> u64 {
        self.submitted
    }
}

#[derive(Debug, Default)]
struct SignatureVerificationPoolMetrics {
    submitted: AtomicU64,
    completed: AtomicU64,
    invalid: AtomicU64,
    cancelled: AtomicU64,
    worker_panics: AtomicU64,
    worker_unavailable: AtomicU64,
    queue_full: AtomicU64,
    queue_closed: AtomicU64,
    header_standard_caches_prepared: AtomicU64,
    header_unsupported_witness_fallbacks: AtomicU64,
    header_preverified_ecdsa_operations: AtomicU64,
    header_canonical_cache_consumptions: AtomicU64,
    header_canonical_cache_lookups: AtomicU64,
    header_canonical_cache_hits: AtomicU64,
    header_canonical_cache_misses: AtomicU64,
}

impl SignatureVerificationPoolMetrics {
    fn record_result<R>(&self, result: &Result<R, SignatureVerificationError>) {
        match result {
            Ok(_) => {
                self.completed.fetch_add(1, Ordering::Relaxed);
            }
            Err(SignatureVerificationError::InvalidWitness(_)) => {
                self.invalid.fetch_add(1, Ordering::Relaxed);
            }
            Err(SignatureVerificationError::Cancelled) => {
                self.cancelled.fetch_add(1, Ordering::Relaxed);
            }
            Err(SignatureVerificationError::WorkerPanicked) => {
                self.worker_panics.fetch_add(1, Ordering::Relaxed);
            }
            Err(SignatureVerificationError::WorkerUnavailable) => {
                self.worker_unavailable.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn snapshot(&self) -> SignatureVerificationPoolMetricsSnapshot {
        SignatureVerificationPoolMetricsSnapshot {
            submitted: self.submitted.load(Ordering::Relaxed),
            completed: self.completed.load(Ordering::Relaxed),
            invalid: self.invalid.load(Ordering::Relaxed),
            cancelled: self.cancelled.load(Ordering::Relaxed),
            worker_panics: self.worker_panics.load(Ordering::Relaxed),
            worker_unavailable: self.worker_unavailable.load(Ordering::Relaxed),
            queue_full: self.queue_full.load(Ordering::Relaxed),
            queue_closed: self.queue_closed.load(Ordering::Relaxed),
            header_standard_caches_prepared: self
                .header_standard_caches_prepared
                .load(Ordering::Relaxed),
            header_unsupported_witness_fallbacks: self
                .header_unsupported_witness_fallbacks
                .load(Ordering::Relaxed),
            header_preverified_ecdsa_operations: self
                .header_preverified_ecdsa_operations
                .load(Ordering::Relaxed),
            header_canonical_cache_consumptions: self
                .header_canonical_cache_consumptions
                .load(Ordering::Relaxed),
            header_canonical_cache_lookups: self
                .header_canonical_cache_lookups
                .load(Ordering::Relaxed),
            header_canonical_cache_hits: self.header_canonical_cache_hits.load(Ordering::Relaxed),
            header_canonical_cache_misses: self
                .header_canonical_cache_misses
                .load(Ordering::Relaxed),
        }
    }
}

/// A completed verification ticket.
pub struct SignatureVerificationTicket<R = Option<HeaderSignaturePreverification>> {
    receiver: Receiver<Result<R, SignatureVerificationError>>,
    metrics: Arc<SignatureVerificationPoolMetrics>,
}

impl<R> std::fmt::Debug for SignatureVerificationTicket<R> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SignatureVerificationTicket(..)")
    }
}

impl<R> SignatureVerificationTicket<R> {
    /// Waits for the worker result.  This is intentionally blocking: callers
    /// use it at the ordered publication fence, after work has overlapped.
    pub fn wait(self) -> Result<R, SignatureVerificationError> {
        match self.receiver.recv() {
            Ok(result) => result,
            Err(_) => {
                self.metrics
                    .worker_unavailable
                    .fetch_add(1, Ordering::Relaxed);
                Err(SignatureVerificationError::WorkerUnavailable)
            }
        }
    }
}

/// A receipt for a transaction whose complete signer witness set used only
/// state-independent standard signature scripts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransactionSignatureVerificationReceipt {
    transaction_hash: UInt256,
    transaction_digest: UInt256,
    network_magic: u32,
    chain_spec_id: UInt256,
}

impl TransactionSignatureVerificationReceipt {
    /// Transaction hash covered by this receipt.
    #[must_use]
    pub const fn transaction_hash(&self) -> UInt256 {
        self.transaction_hash
    }

    /// Checks that the receipt still covers the exact transaction and chain
    /// identity at the publication fence.
    #[must_use]
    pub fn matches(&self, transaction: &Transaction, settings: &ProtocolSettings) -> bool {
        transaction.try_hash().ok().is_some_and(|hash| {
            self.transaction_hash == hash
                && transaction_digest(transaction) == Some(self.transaction_digest)
                && self.network_magic == settings.network
                && self.chain_spec_id == protocol_settings_identity_digest(settings)
        })
    }
}

/// Ticket returned for a transaction signature job.
pub type TransactionSignatureVerificationTicket =
    SignatureVerificationTicket<TransactionSignatureVerificationReceipt>;

/// Exact-input advisory cache produced for one standard header witness.
///
/// This value proves only that its cached ECDSA outcomes were computed for the
/// identified header, witness, and network. It cannot authorize the
/// header: the canonical witness stage must still execute NeoVM and apply all
/// consensus checks before publication.
#[derive(Clone, Debug)]
pub struct HeaderSignaturePreverification {
    block_hash: UInt256,
    block_index: u32,
    network_magic: u32,
    witness_digest: UInt256,
    signature_cache: Arc<PreverifiedSignatureCache>,
}

impl HeaderSignaturePreverification {
    /// Header hash covered by this preverification.
    #[must_use]
    pub const fn block_hash(&self) -> UInt256 {
        self.block_hash
    }

    /// Header height covered by this preverification.
    #[must_use]
    pub const fn block_index(&self) -> u32 {
        self.block_index
    }

    /// Network magic included in the exact Neo sign data.
    #[must_use]
    pub const fn network_magic(&self) -> u32 {
        self.network_magic
    }

    /// Digest of the complete invocation and verification scripts.
    #[must_use]
    pub const fn witness_digest(&self) -> UInt256 {
        self.witness_digest
    }

    /// Returns the immutable exact-input ECDSA cache for canonical NeoVM.
    #[must_use]
    pub fn signature_cache(&self) -> Arc<PreverifiedSignatureCache> {
        Arc::clone(&self.signature_cache)
    }

    /// Checks that this advisory cache still covers the exact current input.
    #[must_use]
    pub fn matches(&self, header: &Header, settings: &ProtocolSettings) -> bool {
        let Some(header_hash) = header.try_hash().ok() else {
            return false;
        };
        let Some(header_witness_digest) = witness_digest(&header.witness) else {
            return false;
        };
        self.block_hash == header_hash
            && self.block_index == header.index()
            && self.network_magic == settings.network
            && self.witness_digest == header_witness_digest
    }
}

/// Ticket returned for advisory header signature preverification.
pub type HeaderSignaturePreverificationTicket =
    SignatureVerificationTicket<Option<HeaderSignaturePreverification>>;

/// A bounded pool for optimistic verification work.
pub struct SignatureVerificationPool {
    sender: Option<SyncSender<Job>>,
    workers: Mutex<Vec<JoinHandle<()>>>,
    config: SignatureVerificationPoolConfig,
    metrics: Arc<SignatureVerificationPoolMetrics>,
    #[cfg(test)]
    submit_attempts: AtomicU64,
    #[cfg(test)]
    forced_queue_full_attempts: Mutex<VecDeque<u64>>,
}

impl std::fmt::Debug for SignatureVerificationPool {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SignatureVerificationPool")
            .field("workers", &self.config.workers)
            .field("queue_capacity", &self.config.queue_capacity)
            .finish()
    }
}

impl SignatureVerificationPool {
    /// Starts a bounded pool.  Configuration is validated before threads are
    /// created so malformed node configuration fails during startup.
    pub fn new(
        config: SignatureVerificationPoolConfig,
    ) -> Result<Self, SignatureVerificationPoolConfigError> {
        config.validate()?;
        let (sender, receiver) = mpsc::sync_channel::<Job>(config.queue_capacity);
        let receiver = Arc::new(Mutex::new(receiver));
        let mut workers: Vec<JoinHandle<()>> = Vec::with_capacity(config.workers);
        for worker_index in 0..config.workers {
            let receiver = Arc::clone(&receiver);
            let name = format!("neo-signature-{worker_index}");
            let handle = match thread::Builder::new().name(name).spawn(move || {
                loop {
                    let job = receiver.lock().ok().and_then(|guard| guard.recv().ok());
                    let Some(job) = job else {
                        break;
                    };
                    job();
                }
            }) {
                Ok(handle) => handle,
                Err(_) => {
                    // Wake workers created earlier in this loop before joining
                    // them; otherwise they would remain blocked on `recv`.
                    drop(sender);
                    for worker in workers {
                        let _ = worker.join();
                    }
                    return Err(SignatureVerificationPoolConfigError::WorkerSpawnFailed);
                }
            };
            workers.push(handle);
        }
        Ok(Self {
            sender: Some(sender),
            workers: Mutex::new(workers),
            config,
            metrics: Arc::new(SignatureVerificationPoolMetrics::default()),
            #[cfg(test)]
            submit_attempts: AtomicU64::new(0),
            #[cfg(test)]
            forced_queue_full_attempts: Mutex::new(VecDeque::new()),
        })
    }

    /// Bounded caller-side window for outstanding tickets.
    #[must_use]
    pub const fn window(&self) -> usize {
        self.config.window()
    }

    /// Returns a point-in-time snapshot of pool counters.
    #[must_use]
    pub fn metrics_snapshot(&self) -> SignatureVerificationPoolMetricsSnapshot {
        self.metrics.snapshot()
    }

    /// Aggregates one cache's delta after a canonical header-verification fence.
    ///
    /// A cache that was prepared but never installed on an application engine
    /// contributes nothing. This keeps unsupported, cancelled, mismatched, and
    /// validation-short-circuited work out of canonical-use telemetry.
    pub(crate) fn record_header_cache_consumption(
        &self,
        cache: &PreverifiedSignatureCache,
        before: PreverifiedSignatureCacheMetricsSnapshot,
    ) {
        let after = cache.metrics_snapshot();
        let consumptions = after.canonical_uses.saturating_sub(before.canonical_uses);
        if consumptions == 0 {
            return;
        }
        self.metrics
            .header_canonical_cache_consumptions
            .fetch_add(consumptions, Ordering::Relaxed);
        self.metrics.header_canonical_cache_lookups.fetch_add(
            after.lookups.saturating_sub(before.lookups),
            Ordering::Relaxed,
        );
        self.metrics
            .header_canonical_cache_hits
            .fetch_add(after.hits.saturating_sub(before.hits), Ordering::Relaxed);
        self.metrics.header_canonical_cache_misses.fetch_add(
            after.misses.saturating_sub(before.misses),
            Ordering::Relaxed,
        );
    }

    /// Schedules one arbitrary typed verification job.
    fn try_submit<R, F>(
        &self,
        job: F,
    ) -> Result<SignatureVerificationTicket<R>, SignatureVerificationSubmitError>
    where
        R: Send + 'static,
        F: FnOnce() -> Result<R, SignatureVerificationError> + Send + 'static,
    {
        #[cfg(test)]
        if self.should_force_queue_full() {
            self.metrics.queue_full.fetch_add(1, Ordering::Relaxed);
            return Err(SignatureVerificationSubmitError::QueueFull);
        }

        let (result_tx, result_rx) = mpsc::sync_channel(1);
        let metrics = Arc::clone(&self.metrics);
        let wrapped: Job = Box::new(move || {
            let result = catch_unwind(AssertUnwindSafe(job))
                .map_err(|_| SignatureVerificationError::WorkerPanicked)
                .and_then(|result| result);
            metrics.record_result(&result);
            let _ = result_tx.send(result);
        });
        let Some(sender) = &self.sender else {
            self.metrics.queue_closed.fetch_add(1, Ordering::Relaxed);
            return Err(SignatureVerificationSubmitError::Closed);
        };
        match sender.try_send(wrapped) {
            Ok(()) => {
                self.metrics.submitted.fetch_add(1, Ordering::Relaxed);
                Ok(SignatureVerificationTicket {
                    receiver: result_rx,
                    metrics: Arc::clone(&self.metrics),
                })
            }
            Err(TrySendError::Full(_)) => {
                self.metrics.queue_full.fetch_add(1, Ordering::Relaxed);
                Err(SignatureVerificationSubmitError::QueueFull)
            }
            Err(TrySendError::Disconnected(_)) => {
                self.metrics.queue_closed.fetch_add(1, Ordering::Relaxed);
                Err(SignatureVerificationSubmitError::Closed)
            }
        }
    }

    /// Submits a deterministic blocking job for crate-level queue/backpressure tests.
    #[cfg(test)]
    pub(crate) fn try_submit_for_test<R, F>(
        &self,
        job: F,
    ) -> Result<SignatureVerificationTicket<R>, SignatureVerificationSubmitError>
    where
        R: Send + 'static,
        F: FnOnce() -> Result<R, SignatureVerificationError> + Send + 'static,
    {
        self.try_submit(job)
    }

    /// Forces selected one-based submission attempts to report queue pressure.
    #[cfg(test)]
    pub(crate) fn force_queue_full_on_submit_attempts(&self, attempts: &[u64]) {
        self.submit_attempts.store(0, Ordering::Relaxed);
        let mut forced = self
            .forced_queue_full_attempts
            .lock()
            .expect("forced queue-full schedule lock");
        forced.clear();
        forced.extend(attempts.iter().copied());
    }

    #[cfg(test)]
    fn should_force_queue_full(&self) -> bool {
        let attempt = self.submit_attempts.fetch_add(1, Ordering::Relaxed) + 1;
        let mut forced = self
            .forced_queue_full_attempts
            .lock()
            .expect("forced queue-full schedule lock");
        if forced.front().copied() != Some(attempt) {
            return false;
        }
        forced.pop_front();
        true
    }

    fn try_submit_cancellable<R, F>(
        &self,
        cancelled: Arc<AtomicBool>,
        job: F,
    ) -> Result<SignatureVerificationTicket<R>, SignatureVerificationSubmitError>
    where
        R: Send + 'static,
        F: FnOnce() -> Result<R, SignatureVerificationError> + Send + 'static,
    {
        self.try_submit(move || {
            if cancelled.load(Ordering::Acquire) {
                return Err(SignatureVerificationError::Cancelled);
            }
            job()
        })
    }

    /// Schedules cancellable, state-independent ECDSA preverification for a
    /// header witness.
    ///
    /// Hashing and identity binding happen before queue admission. The worker
    /// reads no provider or storage snapshot and runs no NeoVM code.
    pub(crate) fn try_submit_header_witness_cancellable(
        &self,
        header: Header,
        settings: Arc<ProtocolSettings>,
        cancellation: &SignatureVerificationCancellation,
    ) -> Result<HeaderSignaturePreverificationTicket, SignatureVerificationSubmitError> {
        let block_hash = header
            .try_hash()
            .map_err(|_| SignatureVerificationSubmitError::InvalidInput("header hash"))?;
        let sign_data = neo_payloads::get_sign_data(&header, settings.network)
            .map_err(|_| SignatureVerificationSubmitError::InvalidInput("header sign data"))?;
        let witness_digest = witness_digest(&header.witness).ok_or(
            SignatureVerificationSubmitError::InvalidInput("witness encoding"),
        )?;
        let block_index = header.index();
        let network_magic = settings.network;
        let witness = header.witness;
        let metrics = Arc::clone(&self.metrics);
        let job = move || {
            let preverification = match preverify_standard_witness_signatures(&sign_data, &witness)
            {
                Some(signature_cache) => {
                    metrics
                        .header_standard_caches_prepared
                        .fetch_add(1, Ordering::Relaxed);
                    metrics
                        .header_preverified_ecdsa_operations
                        .fetch_add(signature_cache.operation_count() as u64, Ordering::Relaxed);
                    Some(HeaderSignaturePreverification {
                        block_hash,
                        block_index,
                        network_magic,
                        witness_digest,
                        signature_cache,
                    })
                }
                None => {
                    metrics
                        .header_unsupported_witness_fallbacks
                        .fetch_add(1, Ordering::Relaxed);
                    None
                }
            };
            Ok(preverification)
        };
        self.try_submit_cancellable(cancellation.token(), job)
    }

    /// Schedules state-independent standard transaction signature checks.
    ///
    /// Contract-account and witness-rule transactions are rejected before
    /// queue admission because their verification depends on canonical state.
    /// Callers must leave those transactions on the ordinary verifier path.
    pub fn try_submit_transaction_state_independent(
        &self,
        transaction: Arc<Transaction>,
        settings: Arc<ProtocolSettings>,
    ) -> Result<TransactionSignatureVerificationTicket, SignatureVerificationSubmitError> {
        if !neo_mempool::transaction_witnesses_are_state_independent(&transaction) {
            return Err(SignatureVerificationSubmitError::InvalidInput(
                "transaction witness requires state-dependent verification",
            ));
        }
        let transaction_hash = transaction
            .try_hash()
            .map_err(|_| SignatureVerificationSubmitError::InvalidInput("transaction hash"))?;
        let transaction_digest = transaction_digest(&transaction).ok_or(
            SignatureVerificationSubmitError::InvalidInput("transaction encoding"),
        )?;
        self.try_submit(move || {
            let result = neo_mempool::verify_state_independent(&transaction, &settings);
            if result != neo_primitives::VerifyResult::Succeed {
                return Err(invalid(format!(
                    "transaction signature verification failed: {result:?}"
                )));
            }
            Ok(TransactionSignatureVerificationReceipt {
                transaction_hash,
                transaction_digest,
                network_magic: settings.network,
                chain_spec_id: protocol_settings_identity_digest(&settings),
            })
        })
    }
}

impl Drop for SignatureVerificationPool {
    fn drop(&mut self) {
        // Disconnect workers before joining them.  Jobs already dequeued may
        // finish, but no new work can be admitted after the final Arc drops.
        self.sender.take();
        if let Ok(mut workers) = self.workers.lock() {
            for worker in workers.drain(..) {
                let _ = worker.join();
            }
        }
    }
}

fn invalid(reason: impl Into<String>) -> SignatureVerificationError {
    SignatureVerificationError::InvalidWitness(reason.into())
}

fn witness_digest(witness: &Witness) -> Option<UInt256> {
    let mut writer = BinaryWriter::new();
    witness.serialize(&mut writer).ok()?;
    Some(UInt256::from(Crypto::sha256(&writer.into_bytes())))
}

fn transaction_digest(transaction: &Transaction) -> Option<UInt256> {
    let mut writer = BinaryWriter::new();
    transaction.serialize(&mut writer).ok()?;
    Some(UInt256::from(Crypto::sha256(&writer.into_bytes())))
}

fn protocol_settings_identity_digest(settings: &ProtocolSettings) -> UInt256 {
    let mut bytes = Vec::with_capacity(256);
    bytes.extend_from_slice(&settings.network.to_le_bytes());
    bytes.push(settings.address_version);
    bytes.extend_from_slice(&settings.validators_count.to_le_bytes());
    bytes.extend_from_slice(&settings.milliseconds_per_block.to_le_bytes());
    bytes.extend_from_slice(&settings.max_valid_until_block_increment.to_le_bytes());
    bytes.extend_from_slice(&settings.max_transactions_per_block.to_le_bytes());
    bytes.extend_from_slice(&settings.max_block_size.to_le_bytes());
    bytes.extend_from_slice(&settings.max_traceable_blocks.to_le_bytes());
    bytes.extend_from_slice(&settings.initial_gas_distribution.to_le_bytes());
    bytes.extend_from_slice(&(settings.standby_committee.len() as u32).to_le_bytes());
    for key in &settings.standby_committee {
        let key_bytes = key.as_bytes();
        bytes.extend_from_slice(&(key_bytes.len() as u32).to_le_bytes());
        bytes.extend_from_slice(key_bytes);
    }
    for hardfork in Hardfork::ALL {
        let defined = settings.is_hardfork_defined(hardfork);
        bytes.push(defined as u8);
        if defined {
            let mut low = 0u32;
            let mut high = u32::MAX;
            while low < high {
                let midpoint = low + (high - low) / 2;
                if settings.is_hardfork_enabled(hardfork, midpoint) {
                    high = midpoint;
                } else {
                    low = midpoint.saturating_add(1);
                }
            }
            bytes.extend_from_slice(&low.to_le_bytes());
        }
    }
    UInt256::from(Crypto::sha256(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_config::ProtocolSettings;
    use neo_payloads::{Signer, Transaction, Witness};
    use neo_primitives::{UInt160, WitnessScope};

    fn test_header(witness: Witness) -> Header {
        Header::from_parts(
            0,
            UInt256::zero(),
            UInt256::zero(),
            20,
            0,
            1,
            0,
            witness.script_hash(),
            witness,
        )
    }

    fn standard_header(settings: &ProtocolSettings, signature: Option<[u8; 64]>) -> Header {
        let private = [7u8; 32];
        let public = neo_crypto::Secp256r1Crypto::derive_public_key(&private).expect("public key");
        let verification =
            neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(&public);
        let mut header = test_header(Witness::new_with_scripts(Vec::new(), verification.clone()));
        let sign_data = neo_payloads::get_sign_data(&header, settings.network).expect("sign data");
        let signature = signature.unwrap_or_else(|| {
            neo_crypto::Secp256r1Crypto::sign(&sign_data, &private).expect("signature")
        });
        let mut invocation = vec![neo_vm::OpCode::PUSHDATA1.byte(), 64];
        invocation.extend_from_slice(&signature);
        header.witness = Witness::new_with_scripts(invocation, verification);
        header
    }

    fn standard_transaction_with_signature(signature: [u8; 64]) -> Transaction {
        let verification =
            neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(
                &[2u8; 33],
            );
        let mut invocation = vec![neo_vm::OpCode::PUSHDATA1.byte(), 64];
        invocation.extend_from_slice(&signature);
        let account = UInt160::from_script(&verification);
        let mut transaction = Transaction::new();
        transaction.set_script(vec![neo_vm::OpCode::PUSH1.byte()]);
        transaction.set_signers(vec![Signer::new(account, WitnessScope::NONE)]);
        transaction.set_witnesses(vec![Witness::new_with_scripts(invocation, verification)]);
        transaction
    }

    fn valid_standard_transaction() -> Transaction {
        let private = [7u8; 32];
        let public = neo_crypto::Secp256r1Crypto::derive_public_key(&private).expect("public key");
        let verification =
            neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(&public);
        let account = UInt160::from_script(&verification);
        let mut transaction = Transaction::new();
        transaction.set_script(vec![neo_vm::OpCode::PUSH1.byte()]);
        transaction.set_signers(vec![Signer::new(account, WitnessScope::NONE)]);
        let hash = transaction.try_hash().expect("transaction hash");
        let mut sign_data = ProtocolSettings::default().network.to_le_bytes().to_vec();
        sign_data.extend_from_slice(&hash.to_bytes());
        let signature = neo_crypto::Secp256r1Crypto::sign(&sign_data, &private).expect("signature");
        let mut invocation = vec![neo_vm::OpCode::PUSHDATA1.byte(), 64];
        invocation.extend_from_slice(&signature);
        transaction.set_witnesses(vec![Witness::new_with_scripts(invocation, verification)]);
        transaction
    }

    fn assert_no_canonical_header_cache_consumption(
        metrics: SignatureVerificationPoolMetricsSnapshot,
    ) {
        assert_eq!(metrics.header_canonical_cache_consumptions, 0);
        assert_eq!(metrics.header_canonical_cache_lookups, 0);
        assert_eq!(metrics.header_canonical_cache_hits, 0);
        assert_eq!(metrics.header_canonical_cache_misses, 0);
    }

    #[test]
    fn pool_rejects_unbounded_configuration() {
        assert_eq!(
            SignatureVerificationPoolConfig {
                workers: 0,
                queue_capacity: 1,
            }
            .validate(),
            Err(SignatureVerificationPoolConfigError::ZeroWorkers)
        );
        assert_eq!(
            SignatureVerificationPoolConfig {
                workers: 1,
                queue_capacity: 0,
            }
            .validate(),
            Err(SignatureVerificationPoolConfigError::ZeroQueue)
        );
    }

    #[test]
    fn worker_panic_is_reported_without_unwinding_caller() {
        let pool = SignatureVerificationPool::new(SignatureVerificationPoolConfig {
            workers: 1,
            queue_capacity: 1,
        })
        .expect("pool");
        let ticket = pool
            .try_submit(|| -> Result<u8, SignatureVerificationError> {
                panic!("test worker panic");
            })
            .expect("ticket");
        assert_eq!(
            ticket.wait(),
            Err(SignatureVerificationError::WorkerPanicked)
        );
        assert_eq!(pool.metrics_snapshot().worker_panics, 1);
    }

    #[test]
    fn metrics_snapshot_records_results_and_queue_admission() {
        let pool = SignatureVerificationPool::new(SignatureVerificationPoolConfig {
            workers: 2,
            queue_capacity: 2,
        })
        .expect("pool");
        let valid = pool.try_submit(|| Ok(7u8)).expect("valid ticket");
        let invalid = pool
            .try_submit(|| -> Result<u8, SignatureVerificationError> {
                Err(SignatureVerificationError::InvalidWitness("invalid".into()))
            })
            .expect("invalid ticket");
        assert!(valid.wait().is_ok());
        assert!(matches!(
            invalid.wait(),
            Err(SignatureVerificationError::InvalidWitness(_))
        ));

        let metrics = pool.metrics_snapshot();
        assert_eq!(metrics.submitted, 2);
        assert_eq!(metrics.accepted(), 2);
        assert_eq!(metrics.completed, 1);
        assert_eq!(metrics.invalid, 1);
        assert_eq!(metrics.worker_panics, 0);
        assert_eq!(metrics.worker_unavailable, 0);
        assert_eq!(metrics.queue_full, 0);
        assert_eq!(metrics.queue_closed, 0);
        assert_eq!(metrics.header_standard_caches_prepared, 0);
        assert_eq!(metrics.header_unsupported_witness_fallbacks, 0);
        assert_eq!(metrics.header_preverified_ecdsa_operations, 0);
        assert_no_canonical_header_cache_consumption(metrics);
    }

    #[test]
    fn metrics_snapshot_records_queue_full_and_closed() {
        let mut pool = SignatureVerificationPool::new(SignatureVerificationPoolConfig {
            workers: 1,
            queue_capacity: 1,
        })
        .expect("pool");
        let (started_tx, started_rx) = mpsc::sync_channel(0);
        let (release_tx, release_rx) = mpsc::sync_channel(0);
        let running = pool
            .try_submit(move || {
                started_tx.send(()).expect("worker started");
                release_rx.recv().expect("release worker");
                Ok(7u8)
            })
            .expect("running ticket");
        started_rx.recv().expect("worker started");
        let queued = pool.try_submit(|| Ok(7u8)).expect("queued ticket");
        assert!(matches!(
            pool.try_submit(|| Ok(7u8)),
            Err(SignatureVerificationSubmitError::QueueFull)
        ));
        release_tx.send(()).expect("release worker");
        assert!(running.wait().is_ok());
        assert!(queued.wait().is_ok());

        pool.sender.take();
        assert!(matches!(
            pool.try_submit(|| Ok(7u8)),
            Err(SignatureVerificationSubmitError::Closed)
        ));

        let metrics = pool.metrics_snapshot();
        assert_eq!(metrics.submitted, 2);
        assert_eq!(metrics.completed, 2);
        assert_eq!(metrics.queue_full, 1);
        assert_eq!(metrics.queue_closed, 1);
    }

    #[test]
    fn ticket_wait_records_worker_unavailable() {
        let pool = SignatureVerificationPool::new(SignatureVerificationPoolConfig {
            workers: 1,
            queue_capacity: 1,
        })
        .expect("pool");
        let metrics = Arc::clone(&pool.metrics);
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        drop(result_tx);
        let ticket = SignatureVerificationTicket::<u8> {
            receiver: result_rx,
            metrics,
        };

        assert_eq!(
            ticket.wait(),
            Err(SignatureVerificationError::WorkerUnavailable)
        );
        assert_eq!(pool.metrics_snapshot().worker_unavailable, 1);
    }

    #[test]
    fn cancelled_queued_job_does_not_invoke_verifier() {
        use std::sync::atomic::AtomicUsize;

        let pool = SignatureVerificationPool::new(SignatureVerificationPoolConfig {
            workers: 1,
            queue_capacity: 2,
        })
        .expect("pool");
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let running = pool
            .try_submit(move || {
                started_tx.send(()).expect("announce running job");
                release_rx.recv().expect("release running job");
                Ok(7u8)
            })
            .expect("running ticket");
        started_rx.recv().expect("worker started");

        let cancelled = Arc::new(AtomicBool::new(false));
        let verifier_calls = Arc::new(AtomicUsize::new(0));
        let calls = Arc::clone(&verifier_calls);
        let queued = pool
            .try_submit_cancellable(Arc::clone(&cancelled), move || {
                calls.fetch_add(1, Ordering::Relaxed);
                Ok(7u8)
            })
            .expect("queued ticket");

        cancelled.store(true, Ordering::Release);
        release_tx.send(()).expect("release worker");
        assert!(running.wait().is_ok());
        assert!(matches!(
            queued.wait(),
            Err(SignatureVerificationError::Cancelled)
        ));
        assert_eq!(verifier_calls.load(Ordering::Relaxed), 0);
        assert_eq!(pool.metrics_snapshot().cancelled, 1);
    }

    #[test]
    fn cancelled_header_job_does_not_record_preverification_work() {
        let settings = Arc::new(ProtocolSettings::default());
        let header = standard_header(&settings, None);
        let pool = SignatureVerificationPool::new(SignatureVerificationPoolConfig {
            workers: 1,
            queue_capacity: 2,
        })
        .expect("pool");
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let running = pool
            .try_submit(move || {
                started_tx.send(()).expect("announce running job");
                release_rx.recv().expect("release running job");
                Ok(())
            })
            .expect("running ticket");
        started_rx.recv().expect("worker started");

        let cancellation = SignatureVerificationCancellation::default();
        let queued = pool
            .try_submit_header_witness_cancellable(header, settings, &cancellation)
            .expect("queued header ticket");
        cancellation.cancel();
        release_tx.send(()).expect("release worker");

        assert!(running.wait().is_ok());
        assert!(matches!(
            queued.wait(),
            Err(SignatureVerificationError::Cancelled)
        ));
        let metrics = pool.metrics_snapshot();
        assert_eq!(metrics.cancelled, 1);
        assert_eq!(metrics.header_standard_caches_prepared, 0);
        assert_eq!(metrics.header_unsupported_witness_fallbacks, 0);
        assert_eq!(metrics.header_preverified_ecdsa_operations, 0);
        assert_no_canonical_header_cache_consumption(metrics);
    }

    #[test]
    fn header_preverification_is_bound_to_exact_header_witness_and_network() {
        let settings = Arc::new(ProtocolSettings::default());
        let header = standard_header(&settings, None);
        let pool = SignatureVerificationPool::new(SignatureVerificationPoolConfig {
            workers: 1,
            queue_capacity: 1,
        })
        .expect("pool");
        let cancellation = SignatureVerificationCancellation::default();
        let preverification = pool
            .try_submit_header_witness_cancellable(
                header.clone(),
                Arc::clone(&settings),
                &cancellation,
            )
            .expect("ticket")
            .wait()
            .expect("worker result")
            .expect("standard witness");

        assert_eq!(
            preverification.block_hash(),
            header.try_hash().expect("header hash")
        );
        assert_eq!(preverification.block_index(), header.index());
        assert_eq!(preverification.network_magic(), settings.network);
        assert_eq!(
            preverification.witness_digest(),
            witness_digest(&header.witness).expect("witness digest")
        );
        assert!(preverification.matches(&header, &settings));
        let first_cache = preverification.signature_cache();
        let second_cache = preverification.signature_cache();
        assert!(Arc::ptr_eq(&first_cache, &second_cache));

        let mut changed_header = header.clone();
        changed_header.set_nonce(1);
        assert!(!preverification.matches(&changed_header, &settings));

        let mut changed_witness = header.clone();
        let verification = changed_witness.witness.verification_script.clone();
        let mut invocation = vec![neo_vm::OpCode::PUSHDATA1.byte(), 64];
        invocation.extend_from_slice(&[1u8; 64]);
        changed_witness.witness = Witness::new_with_scripts(invocation, verification);
        assert!(!preverification.matches(&changed_witness, &settings));

        let mut changed_settings = settings.as_ref().clone();
        changed_settings.network = changed_settings.network.saturating_add(1);
        assert!(!preverification.matches(&header, &changed_settings));

        let metrics = pool.metrics_snapshot();
        assert_eq!(metrics.header_standard_caches_prepared, 1);
        assert_eq!(metrics.header_unsupported_witness_fallbacks, 0);
        assert_eq!(metrics.header_preverified_ecdsa_operations, 1);
        assert_no_canonical_header_cache_consumption(metrics);
    }

    #[test]
    fn unsupported_header_witness_is_an_advisory_fallback() {
        let header = test_header(Witness::new_with_scripts(
            Vec::new(),
            vec![neo_vm::OpCode::PUSH1.byte()],
        ));
        let pool = SignatureVerificationPool::new(SignatureVerificationPoolConfig {
            workers: 1,
            queue_capacity: 1,
        })
        .expect("pool");
        let cancellation = SignatureVerificationCancellation::default();
        let result = pool
            .try_submit_header_witness_cancellable(
                header,
                Arc::new(ProtocolSettings::default()),
                &cancellation,
            )
            .expect("ticket")
            .wait()
            .expect("advisory fallback");

        assert!(result.is_none());
        let metrics = pool.metrics_snapshot();
        assert_eq!(metrics.completed, 1);
        assert_eq!(metrics.invalid, 0);
        assert_eq!(metrics.header_standard_caches_prepared, 0);
        assert_eq!(metrics.header_unsupported_witness_fallbacks, 1);
        assert_eq!(metrics.header_preverified_ecdsa_operations, 0);
        assert_no_canonical_header_cache_consumption(metrics);
    }

    #[test]
    fn invalid_standard_signature_remains_an_advisory_cache_result() {
        let settings = Arc::new(ProtocolSettings::default());
        let header = standard_header(&settings, Some([0u8; 64]));
        let pool = SignatureVerificationPool::new(SignatureVerificationPoolConfig {
            workers: 1,
            queue_capacity: 1,
        })
        .expect("pool");
        let cancellation = SignatureVerificationCancellation::default();
        let preverification = pool
            .try_submit_header_witness_cancellable(
                header.clone(),
                Arc::clone(&settings),
                &cancellation,
            )
            .expect("ticket")
            .wait()
            .expect("worker result")
            .expect("recognized standard witness");

        assert!(preverification.matches(&header, &settings));
        let metrics = pool.metrics_snapshot();
        assert_eq!(metrics.completed, 1);
        assert_eq!(metrics.invalid, 0);
        assert_eq!(metrics.header_standard_caches_prepared, 1);
        assert_eq!(metrics.header_unsupported_witness_fallbacks, 0);
        assert_eq!(metrics.header_preverified_ecdsa_operations, 1);
        assert_no_canonical_header_cache_consumption(metrics);
    }

    #[test]
    fn transaction_job_rejects_invalid_standard_signature() {
        let pool = SignatureVerificationPool::new(SignatureVerificationPoolConfig {
            workers: 1,
            queue_capacity: 1,
        })
        .expect("pool");
        let transaction = Arc::new(standard_transaction_with_signature([0u8; 64]));
        let ticket = pool
            .try_submit_transaction_state_independent(
                Arc::clone(&transaction),
                Arc::new(ProtocolSettings::default()),
            )
            .expect("ticket");
        assert!(matches!(
            ticket.wait(),
            Err(SignatureVerificationError::InvalidWitness(reason))
                if reason.contains("transaction signature verification failed")
        ));
        assert_no_canonical_header_cache_consumption(pool.metrics_snapshot());
    }

    #[test]
    fn transaction_job_skips_state_dependent_witnesses() {
        let pool = SignatureVerificationPool::new(SignatureVerificationPoolConfig {
            workers: 1,
            queue_capacity: 1,
        })
        .expect("pool");
        let mut transaction = Transaction::new();
        transaction.set_script(vec![neo_vm::OpCode::PUSH1.byte()]);
        transaction.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::NONE)]);
        transaction.set_witnesses(vec![Witness::new_with_scripts(
            Vec::new(),
            vec![neo_vm::OpCode::PUSH1.byte()],
        )]);
        assert!(matches!(
            pool.try_submit_transaction_state_independent(
                Arc::new(transaction),
                Arc::new(ProtocolSettings::default()),
            ),
            Err(SignatureVerificationSubmitError::InvalidInput(reason))
                if reason.contains("state-dependent")
        ));
    }

    #[test]
    fn transaction_receipt_rejects_changed_witness_with_same_unsigned_hash() {
        let settings = ProtocolSettings::default();
        let transaction = valid_standard_transaction();
        let receipt = TransactionSignatureVerificationReceipt {
            transaction_hash: transaction.try_hash().expect("transaction hash"),
            transaction_digest: transaction_digest(&transaction).expect("transaction digest"),
            network_magic: settings.network,
            chain_spec_id: protocol_settings_identity_digest(&settings),
        };

        let mut changed = transaction.clone();
        changed.set_witnesses(vec![Witness::new_with_scripts(
            {
                let mut invocation = vec![neo_vm::OpCode::PUSHDATA1.byte(), 64];
                invocation.extend_from_slice(&[1u8; 64]);
                invocation
            },
            changed.witnesses()[0].verification_script().to_vec(),
        )]);
        assert_eq!(
            transaction.try_hash().expect("transaction hash"),
            changed.try_hash().expect("unsigned hash is unchanged")
        );
        assert!(!receipt.matches(&changed, &settings));
        let mut wrong_network = settings.clone();
        wrong_network.network = wrong_network.network.saturating_add(1);
        assert!(!receipt.matches(&transaction, &wrong_network));
    }
}
