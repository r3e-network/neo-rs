//! LogEventArgs - matches C# Neo.SmartContract.LogEventArgs exactly

use crate::{IVerifiable, UInt160};
use std::fmt;
use std::sync::Arc;

/// The EventArgs of ApplicationEngine.Log (matches C# LogEventArgs)
#[derive(Clone)]
pub struct LogEventArgs {
    /// The container that containing the executed script
    pub script_container: Arc<dyn IVerifiable>,

    /// The script hash of the contract that sends the log
    pub script_hash: UInt160,

    /// The message of the log
    pub message: String,
}

impl LogEventArgs {
    /// Initializes a new instance
    pub fn new(container: Arc<dyn IVerifiable>, script_hash: UInt160, message: String) -> Self {
        Self {
            script_container: container,
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
