// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Exceptions.NodeException`.

use thiserror::Error;

/// General node-level exception surfaced by REST controllers.
#[derive(Debug, Error)]
#[error("{message}")]
pub struct NodeException {
    message: String,
}

impl NodeException {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Default for NodeException {
    fn default() -> Self {
        Self::new("Node error")
    }
}
