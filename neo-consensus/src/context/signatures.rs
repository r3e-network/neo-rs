//! Prepare, commit, and change-view payload mutation helpers.
//!
//! Consensus stores signature and view-change payloads by validator index. This
//! module keeps that mutation surface together so validator-index validation
//! and current-view filtering stay consistent across service call sites.

use neo_primitives::UInt256;

use crate::{ChangeViewReason, ConsensusError, ConsensusResult};

use super::ConsensusContext;

impl ConsensusContext {
    /// Adds a prepare response invocation script.
    pub fn add_prepare_response(
        &mut self,
        validator_index: u8,
        invocation_script: Vec<u8>,
        preparation_hash: Option<UInt256>,
    ) -> ConsensusResult<()> {
        if validator_index as usize >= self.validator_count() {
            return Err(ConsensusError::InvalidValidatorIndex(validator_index));
        }
        self.prepare_responses
            .insert(validator_index, invocation_script);
        if let Some(hash) = preparation_hash {
            self.prepare_response_hashes.insert(validator_index, hash);
        }
        Ok(())
    }

    /// Adds a commit signature.
    pub fn add_commit(
        &mut self,
        validator_index: u8,
        view_number: u8,
        signature: Vec<u8>,
    ) -> ConsensusResult<()> {
        if validator_index as usize >= self.validator_count() {
            return Err(ConsensusError::InvalidValidatorIndex(validator_index));
        }
        self.commits.insert(validator_index, signature);
        self.commit_view_numbers
            .insert(validator_index, view_number);
        Ok(())
    }

    /// Adds a change-view request.
    pub fn add_change_view(
        &mut self,
        validator_index: u8,
        new_view: u8,
        reason: ChangeViewReason,
        timestamp: u64,
    ) -> ConsensusResult<()> {
        if validator_index as usize >= self.validator_count() {
            return Err(ConsensusError::InvalidValidatorIndex(validator_index));
        }
        self.change_views
            .insert(validator_index, (new_view, reason));
        self.last_change_view_timestamps
            .insert(validator_index, timestamp);
        Ok(())
    }

    /// Collects current-view commit signatures for block finalization.
    #[must_use]
    pub fn collect_commit_signatures(&self) -> Vec<(u8, Vec<u8>)> {
        self.commits
            .iter()
            .filter(|(idx, _)| {
                self.commit_view_numbers
                    .get(idx)
                    .copied()
                    .unwrap_or(self.view_number)
                    == self.view_number
            })
            .map(|(idx, sig)| (*idx, sig.clone()))
            .collect()
    }
}
