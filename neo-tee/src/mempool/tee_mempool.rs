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

mod decrypt;
mod intake;
mod ordering;

#[cfg(test)]
mod tests;
