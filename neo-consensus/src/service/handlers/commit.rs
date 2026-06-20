use super::super::helpers::InvocationScript;
use super::super::{BlockData, ConsensusEvent, ConsensusService};
use crate::context::ConsensusState;
use crate::messages::ConsensusPayload;
use crate::{ConsensusError, ConsensusResult};
use neo_primitives::UInt256;
use tracing::{debug, info, warn};

impl ConsensusService {
    /// Handles Commit message
    pub(in crate::service) fn on_commit(
        &mut self,
        payload: &ConsensusPayload,
    ) -> ConsensusResult<()> {
        debug!(
            block_index = self.context.block_index,
            validator = payload.validator_index,
            "Received Commit"
        );

        // C# `Commit.Deserialize` reads the first 64 bytes as the block signature.
        // Any trailing bytes remain part of the signed ExtensiblePayload data but are
        // not part of `Commit.Signature`.
        if payload.data.len() < 64 {
            return Err(ConsensusError::InvalidSignatureLength {
                expected: 64,
                got: payload.data.len(),
            });
        }
        let commit_signature = payload.data[..64].to_vec();

        // C# reaches `OnCommitReceived` only after ExtensiblePayload witness
        // verification. Apply that rule before caching commits from any view.
        if payload.witness.is_empty() {
            warn!(
                validator = payload.validator_index,
                "Commit missing witness"
            );
            return Err(ConsensusError::signature_failed("Commit missing witness"));
        }
        let sign_data = self.dbft_sign_data(payload)?;
        if !self.verify_signature(&sign_data, &payload.witness, payload.validator_index) {
            warn!(
                validator = payload.validator_index,
                "Commit witness signature verification failed"
            );
            return Err(ConsensusError::signature_failed(
                "Commit witness signature invalid",
            ));
        }

        // Check if we already have this commit
        if self.context.commits.contains_key(&payload.validator_index) {
            return Err(ConsensusError::AlreadyReceived(payload.validator_index));
        }

        let is_current_view = payload.view_number == self.context.view_number;
        if !is_current_view {
            self.context.add_commit(
                payload.validator_index,
                payload.view_number,
                commit_signature,
            )?;
            self.cache_commit_invocation(payload);
            return Ok(());
        }

        // C# DBFT caches current-view commits that arrive before `EnsureHeader()`
        // can produce sign data, then revalidates them after PrepareRequest fills
        // the header fields.
        let block_hash = match self.context.proposed_block_hash {
            Some(hash) => hash,
            None => {
                self.context.add_commit(
                    payload.validator_index,
                    payload.view_number,
                    commit_signature,
                )?;
                self.cache_commit_invocation(payload);
                return Ok(());
            }
        };

        // dBFT commit signature is a signature over block.GetSignData(network),
        // which is `[network:4][block_hash:32]`.
        let block_sign_data = self.block_sign_data(&block_hash);

        // Verify the commit signature over the block hash.
        if !self.verify_signature(&block_sign_data, &commit_signature, payload.validator_index) {
            warn!(
                validator = payload.validator_index,
                "Commit signature verification failed"
            );
            return Err(ConsensusError::signature_failed("Commit signature invalid"));
        }

        // Add the commit (signature is in the payload data)
        self.context.add_commit(
            payload.validator_index,
            payload.view_number,
            commit_signature,
        )?;
        self.cache_commit_invocation(payload);

        // C# OnCommitReceived: a commit for the current view extends the
        // change-view timer (factor 4).
        if payload.view_number == self.context.view_number {
            self.context.extend_timer_by_factor(4);
        }

        self.check_commits()?;

        Ok(())
    }

    fn block_sign_data(&self, block_hash: &UInt256) -> Vec<u8> {
        let mut block_sign_data = Vec::with_capacity(4 + 32);
        block_sign_data.extend_from_slice(&self.network.to_le_bytes());
        block_sign_data.extend_from_slice(&block_hash.as_bytes());
        block_sign_data
    }

    fn cache_commit_invocation(&mut self, payload: &ConsensusPayload) {
        if !payload.witness.is_empty() {
            self.context.commit_invocations.insert(
                payload.validator_index,
                InvocationScript::invocation_script_from_signature(&payload.witness),
            );
        }
    }

    pub(in crate::service) fn revalidate_current_view_commits(&mut self) {
        let Some(block_hash) = self.context.proposed_block_hash else {
            return;
        };
        let block_sign_data = self.block_sign_data(&block_hash);
        let current_view = self.context.view_number;
        let validators: Vec<u8> = self
            .context
            .commits
            .keys()
            .copied()
            .filter(|idx| self.context.commit_view_numbers.get(idx) == Some(&current_view))
            .collect();

        for validator_index in validators {
            let valid = self
                .context
                .commits
                .get(&validator_index)
                .is_some_and(|signature| {
                    self.verify_signature(&block_sign_data, signature, validator_index)
                });
            if !valid {
                warn!(
                    validator = validator_index,
                    "Removing cached Commit with invalid block signature"
                );
                self.context.commits.remove(&validator_index);
                self.context.commit_view_numbers.remove(&validator_index);
                self.context.commit_invocations.remove(&validator_index);
            }
        }
    }

    /// Checks if we have enough commits to finalize the block
    pub(in crate::service) fn check_commits(&mut self) -> ConsensusResult<()> {
        if !self.context.has_enough_commits() {
            return Ok(());
        }

        if self.context.has_missing_proposed_transactions() {
            debug!(
                block_index = self.context.block_index,
                missing = self
                    .context
                    .proposed_tx_hashes
                    .len()
                    .saturating_sub(self.context.available_tx_hashes.len()),
                "Commit threshold reached before all proposal transactions are available"
            );
            return Ok(());
        }

        if self.context.state == ConsensusState::Committed {
            return Ok(());
        }

        // We have enough commits - block is finalized!
        info!(
            block_index = self.context.block_index,
            commits = self.context.commits.len(),
            "Block committed! Preparing block data for assembly..."
        );

        self.context.state = ConsensusState::Committed;

        // Prepare block data for upper layer to assemble the final Block structure
        let block_data = self.prepare_block_data()?;

        let block_hash = self.context.proposed_block_hash.unwrap_or_default();

        self.send_event(ConsensusEvent::BlockCommitted {
            block_index: self.context.block_index,
            block_hash,
            block_data,
        })?;

        self.running = false;

        Ok(())
    }

    /// Prepares block data for assembly by upper layers.
    ///
    /// This matches C# `DBFTPlugin`'s `CreateBlock()` preparation logic:
    /// 1. Collect M commit signatures from validators
    /// 2. Gather all metadata needed for block construction
    /// 3. Return structured data for upper layer to build Block + multi-sig witness
    ///
    /// The upper layer (neo-node) will:
    /// - Build multi-sig witness from signatures + validator pubkeys
    /// - Fetch actual transactions from mempool
    /// - Construct complete Block structure with header + transactions + witness
    /// - Calculate merkle root and finalize the block
    ///
    /// # Returns
    /// * `Ok(BlockData)` - Complete data for block assembly
    /// * `Err(ConsensusError)` - If data preparation fails
    fn prepare_block_data(&self) -> ConsensusResult<BlockData> {
        // Get validator public keys for multi-sig witness
        let validator_pubkeys: Vec<neo_crypto::ECPoint> = self
            .context
            .validators
            .iter()
            .map(|v| v.public_key.clone())
            .collect();

        // Calculate M (required signatures for consensus)
        let m = self.context.m();

        // Collect commit signatures in validator index order
        let mut signatures: Vec<(u8, Vec<u8>)> = self.context.collect_commit_signatures();
        signatures.sort_by_key(|(idx, _)| *idx);

        if signatures.len() < m {
            return Err(ConsensusError::InsufficientSignatures {
                required: m,
                got: signatures.len(),
            });
        }

        info!(
            block_index = self.context.block_index,
            signatures = signatures.len(),
            required = m,
            validators = validator_pubkeys.len(),
            tx_count = self.context.proposed_tx_hashes.len(),
            "Block data prepared for assembly"
        );

        Ok(BlockData {
            block_index: self.context.block_index,
            timestamp: self.context.proposed_timestamp,
            nonce: self.context.nonce,
            primary_index: self.context.primary_index(),
            transaction_hashes: self.context.proposed_tx_hashes.clone(),
            signatures,
            validator_pubkeys,
            required_signatures: m,
            next_consensus: self.context.next_consensus,
        })
    }
}
