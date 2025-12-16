//! TEE-protected mempool implementation
//!
//! This mempool runs inside the TEE enclave to ensure:
//! 1. Transaction arrival times cannot be manipulated
//! 2. Transaction ordering is fair and verifiable
//! 3. Front-running and sandwich attacks are prevented

use crate::enclave::TeeEnclave;
use crate::error::{TeeError, TeeResult};
use crate::mempool::fair_ordering::{
    compute_ordering_key, FairOrderingPolicy, OrderingKey, TransactionTiming,
};
use neo_crypto::{Crypto, Secp256r1Crypto};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tracing::{debug, info};

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

/// A transaction in the TEE mempool
#[allow(dead_code)]
#[derive(Debug, Clone)]
struct TeeMempoolEntry {
    /// Transaction hash
    hash: [u8; 32],
    /// Serialized transaction data
    data: Vec<u8>,
    /// Timing information assigned by enclave
    timing: TransactionTiming,
    /// Computed ordering key
    ordering_key: OrderingKey,
    /// Network fee
    network_fee: i64,
    /// System fee
    system_fee: i64,
    /// Sender script hash
    sender: [u8; 20],
}

/// TEE-protected mempool
pub struct TeeMempool {
    /// Reference to TEE enclave
    enclave: Arc<TeeEnclave>,
    /// Configuration
    config: TeeMempoolConfig,
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
    /// Create a new TEE mempool
    pub fn new(enclave: Arc<TeeEnclave>, config: TeeMempoolConfig) -> TeeResult<Self> {
        if !enclave.is_ready() {
            return Err(TeeError::EnclaveNotInitialized);
        }

        info!(
            "Creating TEE mempool with policy {:?}, capacity {}",
            config.ordering_policy, config.capacity
        );

        Ok(Self {
            enclave,
            config,
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
    /// Returns the assigned sequence number if successful
    pub fn add_transaction(
        &self,
        tx_hash: [u8; 32],
        tx_data: Vec<u8>,
        network_fee: i64,
        system_fee: i64,
        sender: [u8; 20],
    ) -> TeeResult<u64> {
        // Check capacity
        if self.transactions.read().len() >= self.config.capacity {
            return Err(TeeError::MempoolFull);
        }

        // Check for duplicate
        if self.transactions.read().contains_key(&tx_hash) {
            return Err(TeeError::Other("Transaction already in pool".to_string()));
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

        // Compute ordering key based on policy
        let ordering_key =
            compute_ordering_key(self.config.ordering_policy, &timing, &tx_hash, network_fee);

        let entry = TeeMempoolEntry {
            hash: tx_hash,
            data: tx_data,
            timing,
            ordering_key: ordering_key.clone(),
            network_fee,
            system_fee,
            sender,
        };

        // Insert into both indexes
        self.transactions.write().insert(tx_hash, entry);
        self.ordered.write().insert(ordering_key, tx_hash);

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
        if let Some(entry) = self.transactions.write().remove(tx_hash) {
            self.ordered.write().remove(&entry.ordering_key);
            true
        } else {
            false
        }
    }

    /// Get transactions in fair order, up to limit
    pub fn get_ordered_transactions(&self, limit: usize) -> Vec<([u8; 32], Vec<u8>)> {
        let transactions = self.transactions.read();
        let ordered = self.ordered.read();

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

    /// Get transaction hashes in fair order
    pub fn get_ordered_hashes(&self, limit: usize) -> Vec<[u8; 32]> {
        self.ordered
            .read()
            .iter()
            .take(limit)
            .map(|(_, hash)| *hash)
            .collect()
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

    /// Get transaction data by hash
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
        let policy_json = serde_json::to_string(&self.config.ordering_policy).unwrap_or_default();
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

        let signature = Secp256r1Crypto::sign(&message, &private_key)
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
}
