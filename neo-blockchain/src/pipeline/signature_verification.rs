//! Bounded optimistic signature verification for protocol headers.
//!
//! The pool deliberately owns only verification work.  A ticket is a proof
//! that the exact header witness was checked against a specific parent and
//! cache revision; it is never a permission to mutate canonical state.  The
//! caller must validate the receipt again before publishing a header.

use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use neo_config::{Hardfork, ProtocolSettings};
use neo_crypto::Crypto;
use neo_execution::Helper;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_io::{BinaryWriter, Serializable};
use neo_payloads::{Header, Transaction, Witness};
use neo_primitives::{UInt160, UInt256};
use neo_storage::{CacheRead, DataCache, DataCacheVersion};

use super::consensus_witness_stage::{CONSENSUS_WITNESS_MAX_GAS, ParentHeaderContext};

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

/// Failure from a header-witness verification job.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SignatureVerificationError {
    /// The protocol witness or its parent context is invalid.
    #[error("header witness verification failed: {0}")]
    InvalidWitness(String),
    /// A worker panicked while executing the canonical NeoVM helper.
    #[error("header witness verification worker panicked")]
    WorkerPanicked,
    /// The worker disappeared before returning a result.
    #[error("header witness verification worker became unavailable")]
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
    /// Header hashing or witness encoding failed before queue admission.
    #[error("signature verification job could not be prepared: {0}")]
    InvalidInput(&'static str),
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
    /// Jobs whose canonical verification returned a receipt.
    pub valid: u64,
    /// Jobs rejected by canonical witness verification.
    pub invalid: u64,
    /// Jobs terminated by a worker panic.
    pub worker_panics: u64,
    /// Tickets that observed a worker/result channel becoming unavailable.
    pub worker_unavailable: u64,
    /// Submission attempts rejected because the bounded queue was full.
    pub queue_full: u64,
    /// Submission attempts rejected because the pool was closed.
    pub queue_closed: u64,
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
    valid: AtomicU64,
    invalid: AtomicU64,
    worker_panics: AtomicU64,
    worker_unavailable: AtomicU64,
    queue_full: AtomicU64,
    queue_closed: AtomicU64,
}

impl SignatureVerificationPoolMetrics {
    fn record_result<R>(&self, result: &Result<R, SignatureVerificationError>) {
        match result {
            Ok(_) => {
                self.valid.fetch_add(1, Ordering::Relaxed);
            }
            Err(SignatureVerificationError::InvalidWitness(_)) => {
                self.invalid.fetch_add(1, Ordering::Relaxed);
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
            valid: self.valid.load(Ordering::Relaxed),
            invalid: self.invalid.load(Ordering::Relaxed),
            worker_panics: self.worker_panics.load(Ordering::Relaxed),
            worker_unavailable: self.worker_unavailable.load(Ordering::Relaxed),
            queue_full: self.queue_full.load(Ordering::Relaxed),
            queue_closed: self.queue_closed.load(Ordering::Relaxed),
        }
    }
}

/// A completed verification ticket.
pub struct SignatureVerificationTicket<R = SignatureVerificationReceipt> {
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

/// Typed proof produced by one successful header-witness verification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignatureVerificationReceipt {
    block_hash: UInt256,
    block_index: u32,
    previous_hash: UInt256,
    expected_next_consensus: UInt160,
    network_magic: u32,
    chain_spec_id: UInt256,
    state_independent: bool,
    snapshot_version: DataCacheVersion,
    witness_digest: UInt256,
}

impl SignatureVerificationReceipt {
    /// Header hash covered by this receipt.
    #[must_use]
    pub const fn block_hash(&self) -> UInt256 {
        self.block_hash
    }

    /// Header height covered by this receipt.
    #[must_use]
    pub const fn block_index(&self) -> u32 {
        self.block_index
    }

    /// Parent hash used during verification.
    #[must_use]
    pub const fn previous_hash(&self) -> UInt256 {
        self.previous_hash
    }

    /// Parent `NextConsensus` used during verification.
    #[must_use]
    pub const fn expected_next_consensus(&self) -> UInt160 {
        self.expected_next_consensus
    }

    /// Stable digest of the chain identity bound to this receipt.
    #[must_use]
    pub const fn chain_spec_id(&self) -> UInt256 {
        self.chain_spec_id
    }

    /// Whether the verified standard signature script is independent of cache
    /// state and can remain valid while canonical execution advances.
    #[must_use]
    pub const fn state_independent(&self) -> bool {
        self.state_independent
    }

    /// Cache revision used during verification.
    #[must_use]
    pub fn snapshot_version(&self) -> &DataCacheVersion {
        &self.snapshot_version
    }

    /// Checks that a receipt can authorize the exact current header context.
    #[must_use]
    pub fn matches(
        &self,
        header: &Header,
        parent: &ParentHeaderContext,
        settings: &ProtocolSettings,
        snapshot_version: &DataCacheVersion,
    ) -> bool {
        let Some(header_hash) = header.try_hash().ok() else {
            return false;
        };
        let Some(header_witness_digest) = witness_digest(&header.witness) else {
            return false;
        };
        let state_independent = witness_is_state_independent(&header.witness);
        self.block_hash == header_hash
            && self.block_index == header.index()
            && self.previous_hash == parent.hash
            && header.prev_hash() == &parent.hash
            && self.expected_next_consensus == parent.next_consensus
            && self.network_magic == settings.network
            && self.chain_spec_id == protocol_settings_identity_digest(settings)
            && self.state_independent == state_independent
            && (self.state_independent || &self.snapshot_version == snapshot_version)
            && self.witness_digest == header_witness_digest
    }
}

/// A bounded pool for optimistic verification work.
pub struct SignatureVerificationPool {
    sender: Option<SyncSender<Job>>,
    workers: Mutex<Vec<JoinHandle<()>>>,
    config: SignatureVerificationPoolConfig,
    metrics: Arc<SignatureVerificationPoolMetrics>,
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

    /// Schedules one arbitrary typed verification job.
    pub fn try_submit<R, F>(
        &self,
        job: F,
    ) -> Result<SignatureVerificationTicket<R>, SignatureVerificationSubmitError>
    where
        R: Send + 'static,
        F: FnOnce() -> Result<R, SignatureVerificationError> + Send + 'static,
    {
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

    /// Schedules verification of a header against an immutable parent and
    /// snapshot.  The worker calls the same `neo-vm` helper as canonical
    /// verification; no alternate crypto or VM implementation is used.
    pub fn try_submit_header_witness<P, B>(
        &self,
        header: Header,
        parent: ParentHeaderContext,
        settings: Arc<ProtocolSettings>,
        snapshot: Arc<DataCache<B>>,
        native_contract_provider: Arc<P>,
    ) -> Result<SignatureVerificationTicket, SignatureVerificationSubmitError>
    where
        P: NativeContractProvider + 'static,
        B: CacheRead,
    {
        let _ = header
            .try_hash()
            .map_err(|_| SignatureVerificationSubmitError::InvalidInput("header hash"))?;
        let _ = witness_digest(&header.witness).ok_or(
            SignatureVerificationSubmitError::InvalidInput("witness encoding"),
        )?;
        self.try_submit(move || {
            verify_header_witness_with_native_provider(
                &header,
                &parent,
                &settings,
                snapshot.as_ref(),
                native_contract_provider,
            )
        })
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

/// Verifies one header witness synchronously using the canonical NeoVM helper.
pub fn verify_header_witness_with_native_provider<P, B>(
    header: &Header,
    parent: &ParentHeaderContext,
    settings: &ProtocolSettings,
    snapshot: &DataCache<B>,
    native_contract_provider: Arc<P>,
) -> Result<SignatureVerificationReceipt, SignatureVerificationError>
where
    P: NativeContractProvider + 'static,
    B: CacheRead,
{
    let expected_index = parent
        .index
        .checked_add(1)
        .ok_or_else(|| invalid("previous block index overflow"))?;
    if expected_index != header.index() {
        return Err(invalid("previous block index mismatch"));
    }
    if i32::from(header.primary_index()) >= settings.validators_count {
        return Err(invalid("primary index outside the active validator set"));
    }
    if parent.hash != *header.prev_hash() {
        return Err(invalid("previous block hash mismatch"));
    }
    if header.timestamp() <= parent.timestamp {
        return Err(invalid("timestamp not after previous block"));
    }

    Helper::verify_witness_with_native_provider(
        header,
        settings,
        snapshot,
        &parent.next_consensus,
        &header.witness,
        CONSENSUS_WITNESS_MAX_GAS,
        native_contract_provider,
    )
    .map_err(|error| invalid(error.to_string()))?;

    let block_hash = header
        .try_hash()
        .map_err(|error| invalid(error.to_string()))?;
    let witness_digest = witness_digest(&header.witness)
        .ok_or_else(|| invalid("witness encoding failed after verification"))?;
    Ok(SignatureVerificationReceipt {
        block_hash,
        block_index: header.index(),
        previous_hash: parent.hash,
        expected_next_consensus: parent.next_consensus,
        network_magic: settings.network,
        chain_spec_id: protocol_settings_identity_digest(settings),
        state_independent: witness_is_state_independent(&header.witness),
        snapshot_version: snapshot.version(),
        witness_digest,
    })
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

fn witness_is_state_independent(witness: &Witness) -> bool {
    !witness.verification_script.is_empty()
        && Helper::is_standard_contract(&witness.verification_script)
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
            // Both the legacy map-backed settings and the fixed schedule
            // expose the same monotone activation predicate. Recovering the
            // threshold keeps receipt identity exact without coupling this
            // crate to either storage representation.
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

/// Small ordered queue helper used by header import callers.
pub(crate) fn drain_signature_ticket(
    pending: &mut VecDeque<(Header, ParentHeaderContext, SignatureVerificationTicket)>,
    settings: &ProtocolSettings,
    snapshot: &DataCache<impl CacheRead>,
    native_contract_provider: Arc<impl NativeContractProvider>,
) -> Option<(Header, SignatureVerificationReceipt)> {
    let (header, parent, ticket) = pending.pop_front()?;
    match ticket.wait() {
        Ok(receipt) if receipt.matches(&header, &parent, settings, &snapshot.version()) => {
            Some((header, receipt))
        }
        // A successful worker result can still be stale when a non-standard
        // witness observed a different cache revision, or when the ordered
        // parent context advanced before the receipt fence. Re-run the
        // canonical verifier before treating the header as invalid; only a
        // failed synchronous verification truncates the accepted prefix.
        Ok(_) => verify_signature_synchronously(
            header,
            parent,
            settings,
            snapshot,
            native_contract_provider,
        ),
        Err(
            SignatureVerificationError::WorkerPanicked
            | SignatureVerificationError::WorkerUnavailable,
        ) => verify_signature_synchronously(
            header,
            parent,
            settings,
            snapshot,
            native_contract_provider,
        ),
        Err(SignatureVerificationError::InvalidWitness(_)) => None,
    }
}

fn verify_signature_synchronously(
    header: Header,
    parent: ParentHeaderContext,
    settings: &ProtocolSettings,
    snapshot: &DataCache<impl CacheRead>,
    native_contract_provider: Arc<impl NativeContractProvider>,
) -> Option<(Header, SignatureVerificationReceipt)> {
    let receipt = verify_header_witness_with_native_provider(
        &header,
        &parent,
        settings,
        snapshot,
        native_contract_provider,
    )
    .ok()?;
    receipt
        .matches(&header, &parent, settings, &snapshot.version())
        .then_some((header, receipt))
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_config::ProtocolSettings;
    use neo_payloads::{Signer, Transaction, Witness};
    use neo_primitives::WitnessScope;

    fn test_parent(witness: &Witness) -> ParentHeaderContext {
        ParentHeaderContext {
            hash: UInt256::zero(),
            index: 0,
            timestamp: 10,
            next_consensus: witness.script_hash(),
        }
    }

    fn test_header(witness: Witness) -> Header {
        test_header_with_primary(witness, 0)
    }

    fn test_header_with_primary(witness: Witness, primary_index: u8) -> Header {
        Header::from_parts(
            0,
            UInt256::zero(),
            UInt256::zero(),
            20,
            0,
            1,
            primary_index,
            witness.script_hash(),
            witness,
        )
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

    fn test_receipt() -> SignatureVerificationReceipt {
        SignatureVerificationReceipt {
            block_hash: UInt256::zero(),
            block_index: 1,
            previous_hash: UInt256::zero(),
            expected_next_consensus: UInt160::zero(),
            network_magic: 0,
            chain_spec_id: UInt256::zero(),
            state_independent: true,
            snapshot_version: DataCache::new(false).version(),
            witness_digest: UInt256::zero(),
        }
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
            .try_submit(
                || -> Result<SignatureVerificationReceipt, SignatureVerificationError> {
                    panic!("test worker panic");
                },
            )
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
        let valid = pool
            .try_submit(|| Ok(test_receipt()))
            .expect("valid ticket");
        let invalid = pool
            .try_submit(
                || -> Result<SignatureVerificationReceipt, SignatureVerificationError> {
                    Err(SignatureVerificationError::InvalidWitness("invalid".into()))
                },
            )
            .expect("invalid ticket");
        assert!(valid.wait().is_ok());
        assert!(matches!(
            invalid.wait(),
            Err(SignatureVerificationError::InvalidWitness(_))
        ));

        let metrics = pool.metrics_snapshot();
        assert_eq!(metrics.submitted, 2);
        assert_eq!(metrics.accepted(), 2);
        assert_eq!(metrics.valid, 1);
        assert_eq!(metrics.invalid, 1);
        assert_eq!(metrics.worker_panics, 0);
        assert_eq!(metrics.worker_unavailable, 0);
        assert_eq!(metrics.queue_full, 0);
        assert_eq!(metrics.queue_closed, 0);
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
                Ok(test_receipt())
            })
            .expect("running ticket");
        started_rx.recv().expect("worker started");
        let queued = pool
            .try_submit(|| Ok(test_receipt()))
            .expect("queued ticket");
        assert!(matches!(
            pool.try_submit(|| Ok(test_receipt())),
            Err(SignatureVerificationSubmitError::QueueFull)
        ));
        release_tx.send(()).expect("release worker");
        assert!(running.wait().is_ok());
        assert!(queued.wait().is_ok());

        pool.sender.take();
        assert!(matches!(
            pool.try_submit(|| Ok(test_receipt())),
            Err(SignatureVerificationSubmitError::Closed)
        ));

        let metrics = pool.metrics_snapshot();
        assert_eq!(metrics.submitted, 2);
        assert_eq!(metrics.valid, 2);
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
        let ticket = SignatureVerificationTicket::<SignatureVerificationReceipt> {
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
    fn receipt_is_bound_to_header_parent_network_state_and_witness() {
        let witness = Witness::new_with_scripts(Vec::new(), vec![neo_vm::OpCode::PUSH1.byte()]);
        let header = test_header(witness.clone());
        let parent = test_parent(&witness);
        let settings = ProtocolSettings::default();
        let snapshot = DataCache::new(false);
        let receipt = verify_header_witness_with_native_provider(
            &header,
            &parent,
            &settings,
            &snapshot,
            Arc::new(neo_native_contracts::StandardNativeProvider::new()),
        )
        .expect("valid witness");
        assert!(receipt.matches(&header, &parent, &settings, &snapshot.version(),));

        let mut changed = header.clone();
        changed.set_nonce(1);
        assert!(!receipt.matches(&changed, &parent, &settings, &snapshot.version(),));
        assert!(!receipt.matches(
            &header,
            &ParentHeaderContext {
                next_consensus: UInt160::zero(),
                ..parent
            },
            &settings,
            &snapshot.version(),
        ));
    }

    #[test]
    fn pool_header_job_uses_the_same_receipt_gate() {
        let witness = Witness::new_with_scripts(Vec::new(), vec![neo_vm::OpCode::PUSH1.byte()]);
        let header = test_header(witness.clone());
        let parent = test_parent(&witness);
        let settings = Arc::new(ProtocolSettings::default());
        let snapshot = Arc::new(DataCache::new(false));
        let pool = SignatureVerificationPool::new(SignatureVerificationPoolConfig {
            workers: 1,
            queue_capacity: 1,
        })
        .expect("pool");
        let ticket = pool
            .try_submit_header_witness(
                header.clone(),
                parent,
                Arc::clone(&settings),
                Arc::clone(&snapshot),
                Arc::new(neo_native_contracts::StandardNativeProvider::new()),
            )
            .expect("ticket");
        let receipt = ticket.wait().expect("valid witness");
        assert!(receipt.matches(&header, &parent, settings.as_ref(), &snapshot.version(),));
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
        let transaction = standard_transaction_with_signature([0u8; 64]);
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
    }

    #[test]
    fn protocol_identity_binds_exact_hardfork_activation_heights() {
        let mut first = ProtocolSettings::default();
        let mut second = first.clone();
        let hardfork = Hardfork::HfAspidochelone;
        first.hardforks.insert(hardfork, 10);
        second.hardforks.insert(hardfork, 11);
        assert_ne!(
            protocol_settings_identity_digest(&first),
            protocol_settings_identity_digest(&second)
        );
    }

    #[test]
    fn stale_success_receipt_is_reverified_before_publication() {
        let witness = Witness::new_with_scripts(Vec::new(), vec![neo_vm::OpCode::PUSH1.byte()]);
        let header = test_header(witness.clone());
        let parent = test_parent(&witness);
        let settings = ProtocolSettings::default();
        let snapshot = DataCache::new(false);
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        result_tx.send(Ok(test_receipt())).expect("stale receipt");
        drop(result_tx);
        let ticket = SignatureVerificationTicket {
            receiver: result_rx,
            metrics: Arc::new(SignatureVerificationPoolMetrics::default()),
        };
        let mut pending = VecDeque::from([(header.clone(), parent, ticket)]);

        let drained = drain_signature_ticket(
            &mut pending,
            &settings,
            &snapshot,
            Arc::new(neo_native_contracts::StandardNativeProvider::new()),
        );

        assert!(drained.is_some());
        assert!(pending.is_empty());
        let reverified = drained.expect("reverified header").0;
        assert_eq!(reverified.index(), header.index());
        assert_eq!(reverified.hash(), header.hash());
        assert_eq!(reverified.prev_hash(), header.prev_hash());
        assert_eq!(reverified.witness, header.witness);
    }

    #[test]
    fn stale_success_receipt_cannot_publish_invalid_witness() {
        let witness = Witness::new_with_scripts(Vec::new(), vec![neo_vm::OpCode::PUSH0.byte()]);
        let header = test_header(witness.clone());
        let parent = test_parent(&witness);
        let settings = ProtocolSettings::default();
        let snapshot = DataCache::new(false);
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        result_tx.send(Ok(test_receipt())).expect("stale receipt");
        drop(result_tx);
        let ticket = SignatureVerificationTicket {
            receiver: result_rx,
            metrics: Arc::new(SignatureVerificationPoolMetrics::default()),
        };
        let mut pending = VecDeque::from([(header, parent, ticket)]);

        assert!(
            drain_signature_ticket(
                &mut pending,
                &settings,
                &snapshot,
                Arc::new(neo_native_contracts::StandardNativeProvider::new()),
            )
            .is_none()
        );
        assert!(pending.is_empty());
    }

    #[test]
    fn receipt_verification_rejects_primary_index_outside_validator_set() {
        let witness = Witness::new_with_scripts(Vec::new(), vec![neo_vm::OpCode::PUSH1.byte()]);
        let header = test_header_with_primary(witness.clone(), u8::MAX);
        let parent = test_parent(&witness);
        let settings = ProtocolSettings::default();
        let snapshot = DataCache::new(false);

        assert!(matches!(
            verify_header_witness_with_native_provider(
                &header,
                &parent,
                &settings,
                &snapshot,
                Arc::new(neo_native_contracts::StandardNativeProvider::new()),
            ),
            Err(SignatureVerificationError::InvalidWitness(reason))
                if reason.contains("primary index")
        ));
    }
}
