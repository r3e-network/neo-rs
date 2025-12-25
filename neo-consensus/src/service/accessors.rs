use crate::context::ConsensusContext;
use crate::{ConsensusError, ConsensusResult};

use super::ConsensusService;

impl ConsensusService {
    /// Returns our validator index, or an error if we're not a validator.
    /// This is a safe alternative to `my_index.unwrap()` for production code.
    #[inline]
    pub(super) fn my_index(&self) -> ConsensusResult<u8> {
        self.context.my_index.ok_or(ConsensusError::NotValidator)
    }

    /// Returns the current context (for testing/debugging)
    pub fn context(&self) -> &ConsensusContext {
        &self.context
    }

    /// Returns the network magic number this service is configured for.
    pub fn network(&self) -> u32 {
        self.network
    }

    /// Returns whether the service is running
    pub fn is_running(&self) -> bool {
        self.running
    }
}
