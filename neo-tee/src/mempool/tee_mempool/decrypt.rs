use super::*;
impl TeeMempool {
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
}
