use super::*;
impl TeeMempool {
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
