use super::*;
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
            "Added transaction {} with sequence {sequence} to batch {batch_id}",
            hex::encode(&tx_hash[..8])
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
}
