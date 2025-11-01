// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Exceptions.ApplicationEngineException`.

use thiserror::Error;

/// Wrapper error raised when the ApplicationEngine encounters a fault.
#[derive(Debug, Error)]
#[error("{message}")]
pub struct ApplicationEngineException {
    message: String,
}

impl ApplicationEngineException {
    /// Creates a new exception with an optional message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Default for ApplicationEngineException {
    fn default() -> Self {
        Self::new("Application engine error")
    }
}
