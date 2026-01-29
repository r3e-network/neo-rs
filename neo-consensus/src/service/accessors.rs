use crate::context::{ConsensusContext, ValidatorInfo};
use crate::{ConsensusError, ConsensusResult, ConsensusSigner};
use std::path::Path;
use std::sync::Arc;

use super::ConsensusService;

impl ConsensusService {
    /// Returns our validator index, or an error if we're not a validator.
    /// This is a safe alternative to `my_index.unwrap()` for production code.
    #[inline]
    pub(super) fn my_index(&self) -> ConsensusResult<u8> {
        self.context.my_index.ok_or(ConsensusError::NotValidator)
    }

    /// Returns the current context (for testing/debugging)
    #[must_use] 
    pub const fn context(&self) -> &ConsensusContext {
        &self.context
    }

    /// Updates the validator set and local validator index.
    pub fn update_validators(&mut self, validators: Vec<ValidatorInfo>, my_index: Option<u8>) {
        self.context.validators = validators;
        self.context.my_index = my_index;
    }

    /// Sets the expected block time (in milliseconds) for timeout calculations.
    pub fn set_expected_block_time(&mut self, expected_block_time_ms: u64) {
        self.context.expected_block_time = expected_block_time_ms;
    }

    /// Updates the private key used for signing consensus messages.
    pub fn set_private_key(&mut self, private_key: Vec<u8>) {
        self.private_key = private_key;
    }

    /// Updates the signer used for consensus messages.
    pub fn set_signer(&mut self, signer: Option<Arc<dyn ConsensusSigner>>) {
        self.signer = signer;
    }

    /// Persists the current consensus context to disk for recovery.
    pub fn save_context(&self, path: &Path) -> ConsensusResult<()> {
        self.context.save(path)
    }

    /// Returns the network magic number this service is configured for.
    #[must_use] 
    pub const fn network(&self) -> u32 {
        self.network
    }

    /// Returns whether the service is running
    #[must_use] 
    pub const fn is_running(&self) -> bool {
        self.running
    }
}
