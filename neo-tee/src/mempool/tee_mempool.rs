//! TEE-protected mempool implementation
//!
//! This mempool runs inside the TEE enclave to ensure:
//! 1. Transaction arrival times cannot be manipulated
//! 2. Transaction ordering is fair and verifiable
//! 3. Front-running and sandwich attacks are prevented

use crate::enclave::TeeEnclave;
use crate::error::{TeeError, TeeResult};
use crate::mempool::crypto::{
    CommitteeEpochKey, ContentCipher, KeyEncapsulation, MempoolCiphertext,
};
use crate::mempool::fair_ordering::{
    FairOrderingPolicy, OrderingKey, TransactionTiming, compute_ordering_key,
    compute_threshold_decrypted_ordering_key,
};
use neo_crypto::{Crypto, Secp256r1Crypto};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tracing::{debug, info, warn};
use zeroize::Zeroize;

/// Configuration for TEE mempool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeeMempoolConfig {
    /// Maximum number of transactions in the pool
    pub capacity: usize,
    /// Fair ordering policy
    pub ordering_policy: FairOrderingPolicy,
    /// How often to create new batches (for batched policies)
    pub batch_interval: Duration,
    /// Whether to encrypt transaction contents until ordering
    pub encrypt_contents: bool,
}

impl Default for TeeMempoolConfig {
    fn default() -> Self {
        Self {
            capacity: 50000,
            ordering_policy: FairOrderingPolicy::default(),
            batch_interval: Duration::from_millis(100),
            encrypt_contents: false,
        }
    }
}

/// Content state of an encrypted-mempool entry (Privacy Mode Phase A).
///
/// The decrypt stage of the encrypted mempool is a fail-closed state machine:
///
/// ```text
/// Encrypted --(decode + decapsulate + AEAD verify OK)--> Resolved
/// Encrypted --(any decode/decapsulation/AEAD failure)--> Rejected
/// ```
///
/// * `Encrypted` — initial state after intake: the host-visible storage
///   holds only the [`MempoolCiphertext`] envelope and the transaction hash.
/// * `Resolved` — payload decrypted and AEAD-verified inside the (simulated)
///   TEE boundary; the entry may enter candidate sets and ordering proofs.
/// * `Rejected` — decryption/decapsulation failed; the entry NEVER enters a
///   candidate set. Rejection is fail-closed and never panics.
///
/// In plaintext mode (`encrypt_contents = false`) entries are created
/// directly as `Resolved` — the content is immediately usable and the
/// default behavior is byte-for-byte identical to the pre-Phase-A mempool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContentState {
    /// Encrypted at intake; not yet decrypted.
    Encrypted,
    /// Decrypted and AEAD-verified; eligible for candidate sets.
    Resolved,
    /// Decryption/decapsulation failed; excluded from candidate sets
    /// (fail-closed), with the recorded rejection reason.
    Rejected {
        /// Human-readable rejection cause (decryption, decapsulation, or
        /// rollback-protection failure).
        reason: String,
    },
}

/// KEM mode selection for mempool content encryption (Phase A).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EncryptionMode {
    /// Simulated Keypers committee: a single epoch key encapsulates the
    /// per-payload data keys (threshold share splitting is future work,
    /// see `crate::mempool::crypto::CommitteeEpochKey`).
    CommitteeKey {
        /// The committee epoch the pool seals contents for.
        epoch_id: u64,
    },
    /// The enclave sealing key encapsulates data keys, protected by the
    /// external rollback floor witness.
    TeeSealedKey,
}

/// Opt-in content-encryption settings (Privacy Mode Phase A).
///
/// Passed to [`TeeMempool::with_content_encryption`]. Strictly opt-in:
/// constructing a mempool with [`TeeMempool::new`] and the default config
/// keeps plaintext storage with zero behavioral change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentEncryptionConfig {
    /// Which KEM mode wraps the per-payload data keys.
    pub mode: EncryptionMode,
}

/// Result of one `resolve_pending()` decrypt pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ResolveSummary {
    /// Entries that decrypted and AEAD-verified successfully (`Resolved`).
    pub resolved: usize,
    /// Entries whose decryption/decapsulation failed (`Rejected`, fail-closed).
    pub rejected: usize,
}

/// A transaction in the TEE mempool
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct TeeMempoolEntry {
    /// Transaction hash
    pub hash: [u8; 32],
    /// Serialized transaction data. In plaintext mode this is the raw
    /// transaction bytes; in encrypted mode this is the encoded
    /// [`MempoolCiphertext`] envelope — the host-visible storage NEVER
    /// contains plaintext payload bytes in encrypted mode.
    pub data: Vec<u8>,
    /// Timing information assigned by enclave
    pub timing: TransactionTiming,
    /// Computed ordering key
    pub ordering_key: OrderingKey,
    /// Network fee
    pub network_fee: i64,
    /// System fee
    pub system_fee: i64,
    /// Sender account hash (first signer)
    pub sender: [u8; 20],
    /// Encrypted-mempool content state machine state (Phase A). Plaintext
    /// entries are created directly as `Resolved`.
    pub content_state: ContentState,
    /// Decrypted payload, present only after a successful
    /// `Encrypted -> Resolved` transition.
    ///
    /// Simulation caveat: in Phase A this lives in ordinary host memory
    /// once resolved; production keeps resolved payloads inside the enclave
    /// boundary only.
    pub resolved_data: Option<Vec<u8>>,
}

/// Internal content-encryption engine handle for an encrypted mempool.
struct ContentEncryption {
    /// KEM provider used to wrap/unwrap per-payload data keys.
    kem: KeyEncapsulation,
}

/// TEE-protected mempool
///
/// # Privacy Mode Phase A: encrypted contents (strictly opt-in)
///
/// With `encrypt_contents = false` (the default) this mempool stores and
/// returns plaintext transaction bytes exactly as before — zero behavioral
/// change.
///
/// With content encryption enabled (see [`TeeMempool::with_content_encryption`]
/// or `TeeMempoolConfig::encrypt_contents = true`), `add_transaction`
/// encrypts the payload through [`crate::mempool::crypto`] before storage:
/// the host-visible pool state contains only ciphertext envelopes plus
/// transaction hashes. Decryption is a separate stage driven by
/// [`TeeMempool::resolve_pending`], which runs the fail-closed
/// [`ContentState`] state machine over all pending entries and re-keys the
/// ordering index for the threshold-encryption policy AFTER decryption.
/// Candidate-set accessors (`get_ordered_hashes`,
/// `get_ordered_transactions`, `generate_ordering_proof`) only ever surface
/// `Resolved` entries while encryption is enabled.
///
/// Simulation caveat: under the `simulation` feature this is interface and
/// flow verification only — host-side encryption does not provide real
/// enclave isolation guarantees. Production requires a real TEE path.
pub struct TeeMempool {
    /// Reference to TEE enclave
    enclave: Arc<TeeEnclave>,
    /// Configuration
    config: TeeMempoolConfig,
    /// Content-encryption engine; `None` means plaintext mode
    /// (`encrypt_contents = false`, the default).
    content_encryption: Option<ContentEncryption>,
    /// Transactions indexed by hash
    transactions: RwLock<HashMap<[u8; 32], TeeMempoolEntry>>,
    /// Transactions sorted by ordering key
    ordered: RwLock<BTreeMap<OrderingKey, [u8; 32]>>,
    /// Current sequence number (monotonic)
    sequence_counter: RwLock<u64>,
    /// Current batch ID
    current_batch: RwLock<u64>,
    /// Last batch creation time
    last_batch_time: RwLock<Instant>,
    /// Attestation proof of ordering (updated periodically)
    ordering_proof: RwLock<Option<OrderingProof>>,
}

/// Cryptographic proof of fair ordering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderingProof {
    /// Merkle root of ordered transaction hashes
    pub merkle_root: [u8; 32],
    /// Timestamp when proof was generated
    pub timestamp: SystemTime,
    /// Enclave monotonic counter at proof time
    pub enclave_counter: u64,
    /// Hash of ordering policy parameters
    pub policy_hash: [u8; 32],
    /// Compressed secp256r1 public key used to sign this proof.
    pub public_key: Vec<u8>,
    /// Signature over (merkle_root || counter || policy_hash).
    pub signature: Vec<u8>,
}

impl TeeMempool {
    /// Create a new TEE mempool.
    ///
    /// With the default config (`encrypt_contents = false`) the mempool is
    /// byte-for-byte identical to the pre-Phase-A behavior (plaintext
    /// storage). If `config.encrypt_contents` is set to `true` through this
    /// constructor, the mempool enables content encryption with the default
    /// `EncryptionMode::TeeSealedKey` KEM (the enclave is already at hand);
    /// use [`TeeMempool::with_content_encryption`] to select the committee
    /// epoch-key KEM explicitly.
    pub fn new(enclave: Arc<TeeEnclave>, config: TeeMempoolConfig) -> TeeResult<Self> {
        let encryption = if config.encrypt_contents {
            Some(ContentEncryption {
                kem: KeyEncapsulation::TeeSealed(enclave.clone()),
            })
        } else {
            None
        };
        Self::with_encryption(enclave, config, encryption)
    }

    /// Create a new TEE mempool with encrypted contents (Privacy Mode Phase A).
    ///
    /// Strictly opt-in: this forces `encrypt_contents = true` and installs
    /// the requested KEM mode:
    ///
    /// * [`EncryptionMode::CommitteeKey`] — the pool generates a fresh random
    ///   epoch key for the configured epoch id (kept in memory only; a
    ///   restart starts a new epoch, and leftovers from a previous epoch
    ///   fail closed at decrypt time);
    /// * [`EncryptionMode::TeeSealedKey`] — data keys are wrapped with the
    ///   enclave sealing key and guarded by the external rollback floor.
    pub fn with_content_encryption(
        enclave: Arc<TeeEnclave>,
        config: TeeMempoolConfig,
        encryption: ContentEncryptionConfig,
    ) -> TeeResult<Self> {
        let kem = match encryption.mode {
            EncryptionMode::CommitteeKey { epoch_id } => {
                KeyEncapsulation::CommitteeKey(CommitteeEpochKey::generate(epoch_id))
            }
            EncryptionMode::TeeSealedKey => KeyEncapsulation::TeeSealed(enclave.clone()),
        };
        let mut config = config;
        config.encrypt_contents = true;
        Self::with_encryption(enclave, config, Some(ContentEncryption { kem }))
    }

    /// Common constructor body shared by the public constructors.
    fn with_encryption(
        enclave: Arc<TeeEnclave>,
        config: TeeMempoolConfig,
        content_encryption: Option<ContentEncryption>,
    ) -> TeeResult<Self> {
        if !enclave.is_ready() {
            return Err(TeeError::EnclaveNotInitialized);
        }

        info!(
            "Creating TEE mempool with policy {:?}, capacity {}, encrypted contents {}",
            config.ordering_policy,
            config.capacity,
            content_encryption.is_some()
        );

        Ok(Self {
            enclave,
            config,
            content_encryption,
            transactions: RwLock::new(HashMap::new()),
            ordered: RwLock::new(BTreeMap::new()),
            sequence_counter: RwLock::new(0),
            current_batch: RwLock::new(0),
            last_batch_time: RwLock::new(Instant::now()),
            ordering_proof: RwLock::new(None),
        })
    }

    /// Add a transaction to the mempool
    ///
    /// Returns the assigned sequence number if successful.
    ///
    /// With content encryption enabled the payload is sealed before storage
    /// and the plaintext buffer is zeroized; encryption failure fails closed
    /// (the transaction is not added).
    pub fn add_transaction(
        &self,
        tx_hash: [u8; 32],
        mut tx_data: Vec<u8>,
        network_fee: i64,
        system_fee: i64,
        sender: [u8; 20],
    ) -> TeeResult<u64> {
        // Encrypted mode: fail early on a guaranteed rejection (capacity /
        // duplicate) BEFORE burning a monotonic counter tick on the KEM.
        // The authoritative checks below are unchanged.
        if self.content_encryption.is_some() {
            let transactions = self.transactions.read();
            if transactions.len() >= self.config.capacity {
                return Err(TeeError::MempoolFull);
            }
            if transactions.contains_key(&tx_hash) {
                return Err(TeeError::Other("Transaction already in pool".to_string()));
            }
        }

        // Assign sequence number atomically
        let sequence = {
            let mut counter = self.sequence_counter.write();
            *counter += 1;
            *counter
        };

        // Check if we need to start a new batch
        self.maybe_rotate_batch();

        // Create timing info
        let batch_id = *self.current_batch.read();
        let timing = TransactionTiming::new(sequence).with_batch(batch_id);

        // Reference fee anchoring the FcfsWithGasCap fee cap: the median
        // network fee across the pool at intake time (0 for an empty pool, in
        // which case the policy degrades to pure FCFS). Read under the shared
        // read lock; the authoritative admission checks below still run under
        // the write lock.
        let reference_fee = {
            let transactions = self.transactions.read();
            if transactions.is_empty() {
                0
            } else {
                let mut fees: Vec<i64> = transactions
                    .values()
                    .map(|entry| entry.network_fee)
                    .collect();
                fees.sort_unstable();
                fees[fees.len() / 2]
            }
        };

        // Compute ordering key based on policy
        let ordering_key = compute_ordering_key(
            self.config.ordering_policy,
            &timing,
            &tx_hash,
            network_fee,
            reference_fee,
        );

        // Phase A: seal the payload at intake when content encryption is
        // enabled. The stored bytes are the ciphertext envelope; the
        // plaintext buffer is zeroized immediately after sealing.
        let (stored_data, content_state) = match &self.content_encryption {
            Some(encryption) => {
                let ciphertext = ContentCipher::seal(&tx_data, &tx_hash, &encryption.kem)?;
                tx_data.zeroize();
                (ciphertext.encode()?, ContentState::Encrypted)
            }
            None => (tx_data, ContentState::Resolved),
        };

        let entry = TeeMempoolEntry {
            hash: tx_hash,
            data: stored_data,
            timing,
            ordering_key: ordering_key.clone(),
            network_fee,
            system_fee,
            sender,
            content_state,
            resolved_data: None,
        };

        // Insert into both indexes atomically under write locks to keep both
        // indexes consistent under concurrent writers.
        let mut transactions = self.transactions.write();
        let mut ordered = self.ordered.write();

        if transactions.len() >= self.config.capacity {
            return Err(TeeError::MempoolFull);
        }

        if transactions.contains_key(&tx_hash) {
            return Err(TeeError::Other("Transaction already in pool".to_string()));
        }

        transactions.insert(tx_hash, entry);
        if let Some(replaced_hash) = ordered.insert(ordering_key, tx_hash)
            && replaced_hash != tx_hash
        {
            // Roll back on unexpected index collision.
            transactions.remove(&tx_hash);
            return Err(TeeError::Other(
                "Ordering index collision while inserting transaction".to_string(),
            ));
        }

        debug!(
            "Added transaction {} with sequence {} to batch {}",
            hex::encode(&tx_hash[..8]),
            sequence,
            batch_id
        );

        Ok(sequence)
    }

    /// Remove a transaction from the mempool
    pub fn remove_transaction(&self, tx_hash: &[u8; 32]) -> bool {
        let mut transactions = self.transactions.write();
        let mut ordered = self.ordered.write();

        if let Some(entry) = transactions.remove(tx_hash) {
            ordered.remove(&entry.ordering_key);
            true
        } else {
            false
        }
    }

    /// Get transactions in fair order, up to limit.
    ///
    /// In encrypted mode only `Resolved` entries are surfaced, and the
    /// payload is the decrypted content (the output of the decrypt stage).
    /// In plaintext mode the behavior is unchanged.
    pub fn get_ordered_transactions(&self, limit: usize) -> Vec<([u8; 32], Vec<u8>)> {
        let transactions = self.transactions.read();
        let ordered = self.ordered.read();

        if self.content_encryption.is_some() {
            ordered
                .values()
                .filter_map(|hash| transactions.get(hash))
                .filter(|entry| entry.content_state == ContentState::Resolved)
                .take(limit)
                .filter_map(|entry| entry.resolved_data.clone().map(|data| (entry.hash, data)))
                .collect()
        } else {
            ordered
                .iter()
                .take(limit)
                .filter_map(|(_, hash)| {
                    transactions
                        .get(hash)
                        .map(|entry| (entry.hash, entry.data.clone()))
                })
                .collect()
        }
    }

    /// Get transaction hashes in fair order.
    ///
    /// In encrypted mode this is the plaintext-hash ordered table produced by
    /// the decrypt stage: only `Resolved` entries are included
    /// (`Encrypted` and `Rejected` entries are never candidates).
    pub fn get_ordered_hashes(&self, limit: usize) -> Vec<[u8; 32]> {
        let ordered = self.ordered.read();

        if self.content_encryption.is_some() {
            let transactions = self.transactions.read();
            ordered
                .iter()
                .filter(|(_, hash)| {
                    transactions
                        .get(*hash)
                        .is_some_and(|entry| entry.content_state == ContentState::Resolved)
                })
                .take(limit)
                .map(|(_, hash)| *hash)
                .collect()
        } else {
            ordered.iter().take(limit).map(|(_, hash)| *hash).collect()
        }
    }

    /// Phase A decrypt stage: resolve every `Encrypted` entry through the
    /// fail-closed [`ContentState`] state machine.
    ///
    /// * `Encrypted -> Resolved`: envelope decodes, the KEM decapsulates the
    ///   data key, and the payload AEAD tag verifies. For the
    ///   [`FairOrderingPolicy::ThresholdEncryption`] policy the definitive
    ///   post-decryption ordering key is assigned here, replacing the
    ///   pre-decryption provisional key, so ordering happens strictly after
    ///   decryption.
    /// * `Encrypted -> Rejected`: any envelope/KEM/AEAD (or rollback-floor)
    ///   failure marks the entry `Rejected` and removes it from the ordering
    ///   index. Rejected entries never enter candidate sets; the call never
    ///   panics and returns a per-pass summary.
    ///
    /// In plaintext mode this is a no-op returning an empty summary.
    pub fn resolve_pending(&self) -> ResolveSummary {
        let Some(encryption) = &self.content_encryption else {
            return ResolveSummary::default();
        };

        let mut transactions = self.transactions.write();
        let mut ordered = self.ordered.write();
        let mut summary = ResolveSummary::default();

        for entry in transactions.values_mut() {
            if entry.content_state != ContentState::Encrypted {
                continue;
            }

            match Self::decrypt_entry(encryption, entry) {
                Ok(plaintext) => {
                    if matches!(
                        self.config.ordering_policy,
                        FairOrderingPolicy::ThresholdEncryption
                    ) {
                        // Threshold-encryption policy: assign the real
                        // ordering key only now that the content is decrypted.
                        let intake_key = std::mem::replace(
                            &mut entry.ordering_key,
                            compute_threshold_decrypted_ordering_key(&entry.timing, &entry.hash),
                        );
                        ordered.remove(&intake_key);
                        ordered.insert(entry.ordering_key.clone(), entry.hash);
                    }
                    entry.resolved_data = Some(plaintext);
                    entry.content_state = ContentState::Resolved;
                    summary.resolved += 1;
                }
                Err(error) => {
                    warn!(
                        tx_hash = %hex::encode(entry.hash),
                        error = %error,
                        "mempool content decryption failed; rejecting entry (fail-closed)"
                    );
                    ordered.remove(&entry.ordering_key);
                    entry.resolved_data = None;
                    entry.content_state = ContentState::Rejected {
                        reason: error.to_string(),
                    };
                    summary.rejected += 1;
                }
            }
        }

        if summary.resolved + summary.rejected > 0 {
            debug!(
                resolved = summary.resolved,
                rejected = summary.rejected,
                "mempool decrypt stage completed"
            );
        }

        summary
    }

    /// Decrypt a single entry's stored envelope.
    ///
    /// Fails closed on any envelope, KEM, or AEAD problem; never panics.
    fn decrypt_entry(
        encryption: &ContentEncryption,
        entry: &TeeMempoolEntry,
    ) -> TeeResult<Vec<u8>> {
        let ciphertext = MempoolCiphertext::decode(&entry.data)?;
        let plaintext = ContentCipher::open(&ciphertext, &entry.hash, &encryption.kem)?;
        Ok(plaintext.to_vec())
    }

    /// Content state of a pool entry (encrypted-mempool state machine).
    pub fn content_state(&self, tx_hash: &[u8; 32]) -> Option<ContentState> {
        self.transactions
            .read()
            .get(tx_hash)
            .map(|e| e.content_state.clone())
    }

    /// Decrypted payload of an entry, available only once `Resolved`.
    ///
    /// Returns `None` for `Encrypted` and `Rejected` entries (fail-closed).
    /// In plaintext mode the stored bytes ARE the resolved payload.
    pub fn get_resolved_transaction(&self, tx_hash: &[u8; 32]) -> Option<Vec<u8>> {
        self.transactions.read().get(tx_hash).and_then(|entry| {
            if entry.content_state != ContentState::Resolved {
                return None;
            }
            match (&entry.resolved_data, self.content_encryption.is_none()) {
                (Some(data), _) => Some(data.clone()),
                // Plaintext mode: the stored data is the payload itself.
                (None, true) => Some(entry.data.clone()),
                (None, false) => None,
            }
        })
    }

    /// Hashes of all `Rejected` entries (decryption/decapsulation failed).
    pub fn rejected_hashes(&self) -> Vec<[u8; 32]> {
        self.transactions
            .read()
            .iter()
            .filter(|(_, entry)| matches!(entry.content_state, ContentState::Rejected { .. }))
            .map(|(hash, _)| *hash)
            .collect()
    }

    /// Drop all `Rejected` entries from the pool, returning how many were
    /// purged. Plain entries are never affected.
    pub fn purge_rejected(&self) -> usize {
        let mut transactions = self.transactions.write();
        let mut ordered = self.ordered.write();

        let rejected: Vec<[u8; 32]> = transactions
            .iter()
            .filter(|(_, entry)| matches!(entry.content_state, ContentState::Rejected { .. }))
            .map(|(hash, _)| *hash)
            .collect();

        for hash in &rejected {
            if let Some(entry) = transactions.remove(hash) {
                ordered.remove(&entry.ordering_key);
            }
        }

        rejected.len()
    }

    /// Get the current ordering proof
    pub fn get_ordering_proof(&self) -> Option<OrderingProof> {
        self.ordering_proof.read().clone()
    }

    /// Generate a new ordering proof
    pub fn generate_ordering_proof(&self) -> TeeResult<OrderingProof> {
        let ordered_hashes = self.get_ordered_hashes(usize::MAX);

        // Build Merkle tree of ordered transactions
        let merkle_root = self.compute_merkle_root(&ordered_hashes);

        // Get enclave counter
        let enclave_counter = self.enclave.current_counter()?;

        // Hash the policy parameters
        let policy_hash = self.hash_policy();

        let (public_key, signature) =
            self.sign_proof(&merkle_root, enclave_counter, &policy_hash)?;

        let proof = OrderingProof {
            merkle_root,
            timestamp: SystemTime::now(),
            enclave_counter,
            policy_hash,
            public_key,
            signature,
        };

        *self.ordering_proof.write() = Some(proof.clone());

        Ok(proof)
    }

    /// Get number of transactions in pool
    pub fn len(&self) -> usize {
        self.transactions.read().len()
    }

    /// Check if pool is empty
    pub fn is_empty(&self) -> bool {
        self.transactions.read().is_empty()
    }

    /// Get pool capacity
    pub fn capacity(&self) -> usize {
        self.config.capacity
    }

    /// Clear all transactions
    pub fn clear(&self) {
        self.transactions.write().clear();
        self.ordered.write().clear();
        info!("TEE mempool cleared");
    }

    /// Check if a transaction exists
    pub fn contains(&self, tx_hash: &[u8; 32]) -> bool {
        self.transactions.read().contains_key(tx_hash)
    }

    /// Get transaction data by hash.
    ///
    /// In encrypted mode this returns the *stored ciphertext envelope*
    /// (which is host-visible by design); use
    /// [`TeeMempool::get_resolved_transaction`] for the decrypted payload
    /// once the entry is `Resolved`. In plaintext mode this is unchanged.
    pub fn get_transaction(&self, tx_hash: &[u8; 32]) -> Option<Vec<u8>> {
        self.transactions
            .read()
            .get(tx_hash)
            .map(|e| e.data.clone())
    }

    /// Get transaction timing info
    pub fn get_timing(&self, tx_hash: &[u8; 32]) -> Option<TransactionTiming> {
        self.transactions
            .read()
            .get(tx_hash)
            .map(|e| e.timing.clone())
    }

    /// Maybe rotate to a new batch based on time
    fn maybe_rotate_batch(&self) {
        let should_rotate = {
            let last_time = self.last_batch_time.read();
            last_time.elapsed() >= self.config.batch_interval
        };

        if should_rotate {
            let mut batch = self.current_batch.write();
            let mut last_time = self.last_batch_time.write();
            *batch += 1;
            *last_time = Instant::now();
            debug!("Rotated to batch {}", *batch);
        }
    }

    /// Compute Merkle root of transaction hashes
    fn compute_merkle_root(&self, hashes: &[[u8; 32]]) -> [u8; 32] {
        if hashes.is_empty() {
            return [0u8; 32];
        }

        let mut current_level: Vec<[u8; 32]> = hashes.to_vec();

        while current_level.len() > 1 {
            let mut next_level = Vec::new();

            for chunk in current_level.chunks(2) {
                let mut hasher = Sha256::new();
                hasher.update(chunk[0]);
                if chunk.len() > 1 {
                    hasher.update(chunk[1]);
                } else {
                    hasher.update(chunk[0]); // Duplicate if odd
                }
                let hash = hasher.finalize();
                let mut result = [0u8; 32];
                result.copy_from_slice(&hash);
                next_level.push(result);
            }

            current_level = next_level;
        }

        current_level[0]
    }

    /// Hash the current ordering policy
    fn hash_policy(&self) -> [u8; 32] {
        // Use a deterministic fallback if serialization fails
        let policy_json = serde_json::to_string(&self.config.ordering_policy)
            .unwrap_or_else(|_| format!("{:?}", self.config.ordering_policy));
        let mut hasher = Sha256::new();
        hasher.update(policy_json.as_bytes());
        let hash = hasher.finalize();
        let mut result = [0u8; 32];
        result.copy_from_slice(&hash);
        result
    }

    /// Sign the ordering proof.
    ///
    /// In `simulation` mode the signing key is deterministically derived from the enclave sealing
    /// key so callers can verify ordering proofs without SGX hardware.
    fn sign_proof(
        &self,
        merkle_root: &[u8; 32],
        counter: u64,
        policy_hash: &[u8; 32],
    ) -> TeeResult<(Vec<u8>, Vec<u8>)> {
        let sealing_key = self.enclave.sealing_key()?;
        let private_key = derive_secp256r1_key_from_sealing_key(&sealing_key)?;
        let public_key = Secp256r1Crypto::derive_public_key(&private_key)
            .map_err(|e| TeeError::Other(format!("Failed to derive proof public key: {e}")))?;

        let mut message = Vec::with_capacity(32 + 8 + 32);
        message.extend_from_slice(merkle_root);
        message.extend_from_slice(&counter.to_le_bytes());
        message.extend_from_slice(policy_hash);

        let signature = Secp256r1Crypto::sign_prehash(&Crypto::sha256(&message), &private_key)
            .map_err(|e| TeeError::Other(format!("Failed to sign proof: {e}")))?;
        Ok((public_key, signature.to_vec()))
    }
}

fn derive_secp256r1_key_from_sealing_key(sealing_key: &[u8; 32]) -> TeeResult<[u8; 32]> {
    // Derive a stable signing key from the sealing key.
    // The `p256` backend rejects invalid scalar values, so probe a few domain-separated hashes.
    for counter in 0u8..16u8 {
        let mut hasher = Sha256::new();
        hasher.update(b"neo-tee-ordering-proof-key-v1");
        hasher.update(sealing_key);
        hasher.update([counter]);
        let candidate = hasher.finalize();

        let mut key = [0u8; 32];
        key.copy_from_slice(&candidate[..32]);
        if Secp256r1Crypto::derive_public_key(&key).is_ok() {
            return Ok(key);
        }
    }

    // Fallback: a final hash with a different domain separator.
    let fallback = Crypto::sha256(b"neo-tee-ordering-proof-key-v1-fallback");
    let mut key = [0u8; 32];
    key.copy_from_slice(&fallback[..32]);
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enclave::EnclaveConfig;
    use tempfile::tempdir;

    fn setup_mempool() -> (tempfile::TempDir, TeeMempool) {
        let temp = tempdir().unwrap();
        let config = EnclaveConfig {
            sealed_data_path: temp.path().to_path_buf(),
            simulation: true,
            ..Default::default()
        };

        let enclave = Arc::new(TeeEnclave::new(config));
        enclave.initialize().unwrap();

        let mempool_config = TeeMempoolConfig::default();
        let mempool = TeeMempool::new(enclave, mempool_config).unwrap();

        (temp, mempool)
    }

    #[test]
    fn test_add_and_retrieve_transactions() {
        let (_temp, mempool) = setup_mempool();

        let tx1_hash = [1u8; 32];
        let tx1_data = vec![0x01, 0x02, 0x03];
        let sender = [0xABu8; 20];

        let seq = mempool
            .add_transaction(tx1_hash, tx1_data.clone(), 1000, 500, sender)
            .unwrap();
        assert_eq!(seq, 1);

        let tx2_hash = [2u8; 32];
        let tx2_data = vec![0x04, 0x05, 0x06];

        let seq = mempool
            .add_transaction(tx2_hash, tx2_data.clone(), 2000, 500, sender)
            .unwrap();
        assert_eq!(seq, 2);

        assert_eq!(mempool.len(), 2);
        assert!(mempool.contains(&tx1_hash));
        assert!(mempool.contains(&tx2_hash));
    }

    #[test]
    fn test_fair_ordering() {
        let (_temp, mempool) = setup_mempool();
        let sender = [0xABu8; 20];

        // Add transactions
        for i in 0..10 {
            let mut hash = [0u8; 32];
            hash[0] = i;
            mempool
                .add_transaction(hash, vec![i], 1000, 500, sender)
                .unwrap();
        }

        let ordered = mempool.get_ordered_hashes(10);
        assert_eq!(ordered.len(), 10);

        // With FCFS policy, first transaction should come first
        // (Note: actual ordering depends on policy and randomness)
    }

    #[test]
    fn test_ordering_proof() {
        let (_temp, mempool) = setup_mempool();
        let sender = [0xABu8; 20];

        for i in 0..5 {
            let mut hash = [0u8; 32];
            hash[0] = i;
            mempool
                .add_transaction(hash, vec![i], 1000, 500, sender)
                .unwrap();
        }

        let proof = mempool.generate_ordering_proof().unwrap();
        assert_ne!(proof.merkle_root, [0u8; 32]);
        assert!(!proof.signature.is_empty());
    }

    #[test]
    fn test_capacity_limit() {
        let temp = tempdir().unwrap();
        let config = EnclaveConfig {
            sealed_data_path: temp.path().to_path_buf(),
            simulation: true,
            ..Default::default()
        };

        let enclave = Arc::new(TeeEnclave::new(config));
        enclave.initialize().unwrap();

        let mempool_config = TeeMempoolConfig {
            capacity: 5,
            ..Default::default()
        };
        let mempool = TeeMempool::new(enclave, mempool_config).unwrap();
        let sender = [0xABu8; 20];

        // Fill to capacity
        for i in 0..5 {
            let mut hash = [0u8; 32];
            hash[0] = i;
            mempool
                .add_transaction(hash, vec![i], 1000, 500, sender)
                .unwrap();
        }

        // Should fail at capacity
        let mut hash = [0u8; 32];
        hash[0] = 100;
        let result = mempool.add_transaction(hash, vec![100], 1000, 500, sender);
        assert!(matches!(result, Err(TeeError::MempoolFull)));
    }

    // ---- Privacy Mode Phase A: encrypted mempool tests ----

    fn setup_enclave(temp: &tempfile::TempDir) -> Arc<TeeEnclave> {
        let config = EnclaveConfig {
            sealed_data_path: temp.path().to_path_buf(),
            simulation: true,
            ..Default::default()
        };
        let enclave = Arc::new(TeeEnclave::new(config));
        enclave.initialize().unwrap();
        enclave
    }

    fn setup_mempool_with(
        mode: EncryptionMode,
        policy: FairOrderingPolicy,
    ) -> (tempfile::TempDir, TeeMempool) {
        let temp = tempdir().unwrap();
        let enclave = setup_enclave(&temp);
        let mempool = TeeMempool::with_content_encryption(
            enclave,
            TeeMempoolConfig {
                ordering_policy: policy,
                ..Default::default()
            },
            ContentEncryptionConfig { mode },
        )
        .unwrap();
        (temp, mempool)
    }

    /// Flip one byte of the stored (host-visible) data of a pool entry.
    fn tamper_stored_data(mempool: &TeeMempool, tx_hash: &[u8; 32], byte_index: usize) {
        let mut transactions = mempool.transactions.write();
        let entry = transactions.get_mut(tx_hash).expect("entry must exist");
        let index = byte_index % entry.data.len();
        entry.data[index] ^= 0xFF;
    }

    /// Transplant a ciphertext produced by another mempool into `target`,
    /// simulating a replay/foreign-blob arriving over the wire.
    fn inject_foreign_ciphertext(target: &TeeMempool, source: &TeeMempool, tx_hash: &[u8; 32]) {
        let data = source.get_transaction(tx_hash).expect("source entry");
        let timing = TransactionTiming::new(1);
        let ordering_key = compute_ordering_key(
            FairOrderingPolicy::ThresholdEncryption,
            &timing,
            tx_hash,
            0,
            0,
        );
        let entry = TeeMempoolEntry {
            hash: *tx_hash,
            data,
            timing,
            ordering_key: ordering_key.clone(),
            network_fee: 0,
            system_fee: 0,
            sender: [0u8; 20],
            content_state: ContentState::Encrypted,
            resolved_data: None,
        };
        target.ordered.write().insert(ordering_key, *tx_hash);
        target.transactions.write().insert(*tx_hash, entry);
    }

    #[test]
    fn test_default_mode_stores_plaintext_bytes_identically() {
        let (_temp, mempool) = setup_mempool();
        // Strictly opt-in: the default pool has no encryption engine at all.
        assert!(mempool.content_encryption.is_none());
        let sender = [0xABu8; 20];

        let payloads: Vec<([u8; 32], Vec<u8>)> = (0..3)
            .map(|i| {
                let mut hash = [0u8; 32];
                hash[0] = i;
                (hash, format!("plaintext-payload-{i}").into_bytes())
            })
            .collect();

        for (hash, payload) in &payloads {
            mempool
                .add_transaction(*hash, payload.clone(), 1000, 500, sender)
                .unwrap();
        }

        // Stored bytes are byte-for-byte identical to the input plaintext.
        for (hash, payload) in &payloads {
            assert_eq!(mempool.get_transaction(hash).as_ref(), Some(payload));
            assert_eq!(mempool.content_state(hash), Some(ContentState::Resolved));
            assert_eq!(
                mempool.get_resolved_transaction(hash),
                Some(payload.clone())
            );
        }

        // Candidate sets return the exact original bytes. (The default
        // policy is BatchedRandom, so ordering is random — compare by hash.)
        let ordered = mempool.get_ordered_transactions(10);
        assert_eq!(ordered.len(), 3);
        let by_hash: HashMap<[u8; 32], Vec<u8>> = ordered.into_iter().collect();
        for (hash, payload) in &payloads {
            assert_eq!(by_hash.get(hash), Some(payload));
        }

        // resolve_pending is a no-op in plaintext mode.
        assert_eq!(mempool.resolve_pending(), ResolveSummary::default());
        assert!(mempool.rejected_hashes().is_empty());

        // Ordering proof still works unchanged.
        let proof = mempool.generate_ordering_proof().unwrap();
        assert_ne!(proof.merkle_root, [0u8; 32]);
    }

    #[test]
    fn test_committee_mode_host_sees_only_ciphertext_then_orders_after_decrypt() {
        let (_temp, mempool) = setup_mempool_with(
            EncryptionMode::CommitteeKey { epoch_id: 7 },
            FairOrderingPolicy::ThresholdEncryption,
        );
        let sender = [0xABu8; 20];

        let payloads: Vec<([u8; 32], Vec<u8>)> = (0..3)
            .map(|i| {
                let mut hash = [0u8; 32];
                hash[0] = i;
                (hash, format!("committee-payload-{i}").into_bytes())
            })
            .collect();
        for (hash, payload) in &payloads {
            mempool
                .add_transaction(*hash, payload.clone(), 1000, 500, sender)
                .unwrap();
        }

        // Host-visible storage: ciphertext envelope only, no plaintext bytes.
        for (hash, payload) in &payloads {
            let stored = mempool.get_transaction(hash).expect("stored envelope");
            assert_ne!(&stored, payload);
            assert!(
                !stored
                    .windows(payload.len())
                    .any(|w| w == payload.as_slice())
            );

            let decoded = MempoolCiphertext::decode(&stored).unwrap();
            assert_eq!(decoded.mode, crate::mempool::crypto::MODE_COMMITTEE_KEY);
            assert_eq!(decoded.epoch_id, 7);

            assert_eq!(mempool.content_state(hash), Some(ContentState::Encrypted));
            assert_eq!(mempool.get_resolved_transaction(hash), None);
        }

        // Pre-resolution candidate sets are empty (decrypt-first state machine).
        assert!(mempool.get_ordered_hashes(10).is_empty());
        assert!(mempool.get_ordered_transactions(10).is_empty());

        // Decrypt stage.
        let summary = mempool.resolve_pending();
        assert_eq!(
            summary,
            ResolveSummary {
                resolved: 3,
                rejected: 0
            }
        );

        // Ordering was assigned AFTER decryption (arrival sequence).
        let ordered_hashes = mempool.get_ordered_hashes(10);
        let want_hashes: Vec<[u8; 32]> = payloads.iter().map(|(h, _)| *h).collect();
        assert_eq!(ordered_hashes, want_hashes);

        // Decrypted payloads come back in the same order, byte-identical.
        let recovered = mempool.get_ordered_transactions(10);
        for ((hash, data), (want_hash, want_payload)) in recovered.iter().zip(&payloads) {
            assert_eq!(hash, want_hash);
            assert_eq!(data, want_payload);
        }

        for (hash, payload) in &payloads {
            assert_eq!(mempool.content_state(hash), Some(ContentState::Resolved));
            assert_eq!(
                mempool.get_resolved_transaction(hash),
                Some(payload.clone())
            );
        }

        // OrderingProof covers the resolved ordered-hash table.
        let proof = mempool.generate_ordering_proof().unwrap();
        assert_ne!(proof.merkle_root, [0u8; 32]);

        // Resolution is idempotent: a second pass resolves nothing.
        assert_eq!(mempool.resolve_pending(), ResolveSummary::default());
    }

    #[test]
    fn test_tee_sealed_mode_round_trip() {
        let (_temp, mempool) = setup_mempool_with(
            EncryptionMode::TeeSealedKey,
            FairOrderingPolicy::FirstComeFirstServed,
        );
        let sender = [0xABu8; 20];

        let mut hash_a = [0x11u8; 32];
        hash_a[0] = 1;
        let mut hash_b = [0x22u8; 32];
        hash_b[0] = 2;
        let payload_a = b"tee-sealed-payload-a".to_vec();
        let payload_b = b"tee-sealed-payload-b".to_vec();

        mempool
            .add_transaction(hash_a, payload_a.clone(), 1000, 500, sender)
            .unwrap();
        mempool
            .add_transaction(hash_b, payload_b.clone(), 2000, 500, sender)
            .unwrap();

        let stored_a = mempool.get_transaction(&hash_a).unwrap();
        assert_ne!(stored_a, payload_a);
        assert!(
            !stored_a
                .windows(payload_a.len())
                .any(|w| w == payload_a.as_slice())
        );
        assert_eq!(
            MempoolCiphertext::decode(&stored_a).unwrap().mode,
            crate::mempool::crypto::MODE_TEE_SEALED_KEY
        );

        let summary = mempool.resolve_pending();
        assert_eq!(
            summary,
            ResolveSummary {
                resolved: 2,
                rejected: 0
            }
        );

        assert_eq!(mempool.get_ordered_hashes(10), vec![hash_a, hash_b]);
        assert_eq!(mempool.get_resolved_transaction(&hash_a), Some(payload_a));
        assert_eq!(mempool.get_resolved_transaction(&hash_b), Some(payload_b));
    }

    #[test]
    fn test_tampered_ciphertext_rejected_fail_closed() {
        let (_temp, mempool) = setup_mempool_with(
            EncryptionMode::TeeSealedKey,
            FairOrderingPolicy::FirstComeFirstServed,
        );
        let sender = [0xABu8; 20];

        let mut good_hash = [0u8; 32];
        good_hash[0] = 1;
        let mut bad_hash = [0u8; 32];
        bad_hash[0] = 2;
        let good_payload = b"good-payload".to_vec();
        let bad_payload = b"tampered-payload".to_vec();

        mempool
            .add_transaction(good_hash, good_payload.clone(), 1000, 500, sender)
            .unwrap();
        mempool
            .add_transaction(bad_hash, bad_payload, 1000, 500, sender)
            .unwrap();

        // Tamper the AEAD tag (last byte of the stored envelope).
        let stored_len = mempool.get_transaction(&bad_hash).unwrap().len();
        tamper_stored_data(&mempool, &bad_hash, stored_len - 1);

        // The decrypt stage must never panic and must fail closed.
        let summary = mempool.resolve_pending();
        assert_eq!(
            summary,
            ResolveSummary {
                resolved: 1,
                rejected: 1
            }
        );

        match mempool.content_state(&bad_hash) {
            Some(ContentState::Rejected { reason }) => {
                assert!(
                    reason.contains("content decryption failed"),
                    "reason: {reason}"
                );
            }
            other => panic!("expected Rejected, got {other:?}"),
        }

        // Fail-closed: the tampered entry never enters a candidate set.
        assert_eq!(mempool.get_ordered_hashes(10), vec![good_hash]);
        assert_eq!(mempool.get_resolved_transaction(&bad_hash), None);
        assert_eq!(mempool.rejected_hashes(), vec![bad_hash]);
        assert_eq!(mempool.len(), 2);

        // Purging removes the rejected entry without touching the good one.
        assert_eq!(mempool.purge_rejected(), 1);
        assert_eq!(mempool.len(), 1);
        assert!(mempool.contains(&good_hash));
        assert!(!mempool.contains(&bad_hash));
        assert_eq!(mempool.get_ordered_hashes(10), vec![good_hash]);
    }

    #[test]
    fn test_tampered_kem_material_rejected() {
        let (_temp, mempool) = setup_mempool_with(
            EncryptionMode::CommitteeKey { epoch_id: 5 },
            FairOrderingPolicy::FirstComeFirstServed,
        );
        let sender = [0xABu8; 20];
        let tx_hash = [0xEEu8; 32];

        mempool
            .add_transaction(tx_hash, b"kem-tamper-payload".to_vec(), 1000, 500, sender)
            .unwrap();

        // Flip a byte inside the committee KEM material (offset 12 is the
        // first KEM-material byte after the fixed envelope header).
        tamper_stored_data(&mempool, &tx_hash, 12);

        let summary = mempool.resolve_pending();
        assert_eq!(summary.rejected, 1);
        assert!(matches!(
            mempool.content_state(&tx_hash),
            Some(ContentState::Rejected { .. })
        ));
        assert!(mempool.get_ordered_hashes(10).is_empty());
        assert_eq!(mempool.get_resolved_transaction(&tx_hash), None);
    }

    #[test]
    fn test_wrong_committee_key_rejected() {
        // Two pools for the same epoch but with independently generated
        // committee keys: a ciphertext from pool A must not decrypt in pool B.
        let (_temp_a, pool_a) = setup_mempool_with(
            EncryptionMode::CommitteeKey { epoch_id: 7 },
            FairOrderingPolicy::ThresholdEncryption,
        );
        let (_temp_b, pool_b) = setup_mempool_with(
            EncryptionMode::CommitteeKey { epoch_id: 7 },
            FairOrderingPolicy::ThresholdEncryption,
        );
        // A third pool on a different epoch must reject it as well.
        let (_temp_c, pool_c) = setup_mempool_with(
            EncryptionMode::CommitteeKey { epoch_id: 8 },
            FairOrderingPolicy::ThresholdEncryption,
        );

        let tx_hash = [0x77u8; 32];
        let payload = b"wrong-key-payload".to_vec();
        pool_a
            .add_transaction(tx_hash, payload, 1000, 500, [0xABu8; 20])
            .unwrap();

        inject_foreign_ciphertext(&pool_b, &pool_a, &tx_hash);
        let summary_b = pool_b.resolve_pending();
        assert_eq!(summary_b.rejected, 1);
        assert_eq!(summary_b.resolved, 0);
        assert!(matches!(
            pool_b.content_state(&tx_hash),
            Some(ContentState::Rejected { .. })
        ));
        assert!(pool_b.get_ordered_hashes(10).is_empty());
        assert_eq!(pool_b.get_resolved_transaction(&tx_hash), None);

        inject_foreign_ciphertext(&pool_c, &pool_a, &tx_hash);
        let summary_c = pool_c.resolve_pending();
        assert_eq!(summary_c.rejected, 1);
        assert!(pool_c.get_ordered_hashes(10).is_empty());
    }

    #[test]
    fn test_tee_sealed_floor_rollback_rejected() {
        let (temp, mempool) = setup_mempool_with(
            EncryptionMode::TeeSealedKey,
            FairOrderingPolicy::FirstComeFirstServed,
        );
        let sender = [0xABu8; 20];

        let mut hash_a = [0u8; 32];
        hash_a[0] = 1;
        let mut hash_b = [0u8; 32];
        hash_b[0] = 2;
        mempool
            .add_transaction(hash_a, b"floor-a".to_vec(), 1000, 500, sender)
            .unwrap();
        mempool
            .add_transaction(hash_b, b"floor-b".to_vec(), 1000, 500, sender)
            .unwrap();

        // Sanity: decapsulation works with an intact floor.
        let before = mempool.resolve_pending();
        assert_eq!(before.resolved, 2);

        // Roll the external floor witness back below the sealed-blob
        // counters: the blobs are no longer covered by floor history.
        let floor_path = crate::enclave::rollback_floor::floor_path(temp.path());
        crate::enclave::rollback_floor::store(&floor_path, 0).unwrap();

        // Reset both entries to Encrypted so the decrypt stage runs again
        // against the rolled-back floor (simulates a restart + replay).
        {
            let mut transactions = mempool.transactions.write();
            for entry in transactions.values_mut() {
                entry.content_state = ContentState::Encrypted;
                entry.resolved_data = None;
            }
        }

        let summary = mempool.resolve_pending();
        assert_eq!(
            summary,
            ResolveSummary {
                resolved: 0,
                rejected: 2
            }
        );
        for hash in [hash_a, hash_b] {
            match mempool.content_state(&hash) {
                Some(ContentState::Rejected { reason }) => {
                    assert!(reason.contains("Rollback protection"), "reason: {reason}");
                }
                other => panic!("expected Rejected, got {other:?}"),
            }
        }
        assert!(mempool.get_ordered_hashes(10).is_empty());
        assert_eq!(mempool.rejected_hashes().len(), 2);
    }
}
