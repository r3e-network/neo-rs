//! Log event arguments emitted by `ApplicationEngine`.

use std::fmt;
use std::sync::Arc;

use neo_primitives::UInt160;

use crate::VerifiableContainer;

/// Event arguments for `ApplicationEngine.Log`.
#[derive(Clone)]
pub struct LogEventArgs {
    /// Script container that emitted the log, when execution has one.
    pub script_container: Option<Arc<VerifiableContainer>>,

    /// Script hash of the contract that emitted the log.
    pub script_hash: UInt160,

    /// Log message.
    pub message: String,
}

impl LogEventArgs {
    /// Creates log event arguments.
    pub fn new(
        container: impl Into<Option<Arc<VerifiableContainer>>>,
        script_hash: UInt160,
        message: String,
    ) -> Self {
        Self {
            script_container: container.into(),
            script_hash,
            message,
        }
    }
}

impl fmt::Debug for LogEventArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LogEventArgs")
            .field("script_hash", &self.script_hash)
            .field("message", &self.message)
            .finish()
    }
}
