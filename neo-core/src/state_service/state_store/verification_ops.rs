use super::{StateRootVerificationResult, StateStore};
use crate::error::{CoreError, CoreResult};
use crate::state_service::StateRoot;
use crate::UInt256;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::debug;

impl StateStore {
    /// Verifies that a computed state root matches the expected root for a given block.
    ///
    /// This is used during block persistence to ensure state consistency.
    ///
    /// # Arguments
    /// * `index` - The block index to verify
    /// * `expected_root` - The expected state root hash
    ///
    /// # Returns
    /// `StateRootVerificationResult` indicating the verification outcome
    pub fn verify_state_root(
        &self,
        index: u32,
        expected_root: &UInt256,
    ) -> StateRootVerificationResult {
        // First check the cache for recent roots
        if let Some(entry) = self.root_cache.write().get(index) {
            if &entry.root_hash() == expected_root {
                return StateRootVerificationResult::Valid;
            } else {
                return StateRootVerificationResult::RootMismatch;
            }
        }

        // Fall back to store lookup
        match self.get_state_root(index) {
            Some(state_root) => {
                if &state_root.root_hash == expected_root {
                    StateRootVerificationResult::Valid
                } else {
                    StateRootVerificationResult::RootMismatch
                }
            }
            None => StateRootVerificationResult::NotFound,
        }
    }

    /// Verifies a state root with full witness validation.
    ///
    /// This performs complete verification including signature checks.
    ///
    /// # Arguments
    /// * `state_root` - The state root to verify
    ///
    /// # Returns
    /// `StateRootVerificationResult` indicating the verification outcome
    pub fn verify_state_root_with_witness(
        &self,
        state_root: &StateRoot,
    ) -> StateRootVerificationResult {
        // Check for witness presence if it's a validated root
        if state_root.witness.is_none() {
            return StateRootVerificationResult::MissingWitness;
        }

        // Verify using the configured verifier
        let Some(verifier) = &self.verifier else {
            return StateRootVerificationResult::VerifierNotConfigured;
        };

        if !verifier.verify(state_root) {
            return StateRootVerificationResult::InvalidWitness;
        }

        StateRootVerificationResult::Valid
    }

    /// Verifies state root consistency during block persistence.
    ///
    /// This method should be called during block persist to ensure the computed
    /// state root matches the expected root from the block or network.
    ///
    /// # Arguments
    /// * `index` - The block index
    /// * `computed_root` - The locally computed state root hash
    /// * `expected_root` - The expected state root hash (from block header or network)
    ///
    /// # Returns
    /// `CoreResult<()>` which is Ok if verification succeeds, Err otherwise
    pub fn verify_state_root_on_persist(
        &self,
        index: u32,
        computed_root: &UInt256,
        expected_root: Option<&UInt256>,
    ) -> CoreResult<()> {
        // Always verify against our locally computed root
        let local_root = match self.local_root_index() {
            Some(idx) if idx == index => self.current_local_root_hash(),
            Some(_) | None => {
                return Err(CoreError::invalid_operation(format!(
                    "Local state root not available for block {}",
                    index
                )));
            }
        };

        let local_root = local_root.ok_or_else(|| {
            CoreError::invalid_operation(format!(
                "Local state root hash not found for block {}",
                index
            ))
        })?;

        if &local_root != computed_root {
            return Err(CoreError::invalid_operation(format!(
                "State root mismatch on persist at block {}: computed={}, local={}",
                index, computed_root, local_root
            )));
        }

        // If an expected root is provided, also verify against it
        if let Some(expected) = expected_root {
            if computed_root != expected {
                return Err(CoreError::invalid_operation(format!(
                    "State root mismatch with expected at block {}: computed={}, expected={}",
                    index, computed_root, expected
                )));
            }
        }

        // Cache the verified root
        let state_root = StateRoot::new_current(index, *computed_root);
        self.root_cache.write().insert_state_root(
            state_root,
            false, // not yet validated by consensus
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        );

        debug!(
            target: "state",
            index,
            root_hash = %computed_root,
            "state root verified and cached on persist"
        );

        Ok(())
    }
}
