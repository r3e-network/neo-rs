// Copyright (C) 2015-2025 The Neo Project.
//
// Rust equivalent of `Neo.Plugins.RpcServer.RpcMethodAttribute`. The original
// attribute marks reflection-based RPC methods; in Rust we represent this as a
// lightweight descriptor that can annotate registered handlers.

use serde::Deserialize;

/// Descriptor for RPC methods with security attributes.
#[derive(Debug, Clone, Deserialize)]
pub struct RpcMethodDescriptor {
    /// The name of the RPC method.
    pub name: String,
    /// Whether this method requires authentication.
    /// If true, the method will be rejected if no authentication is configured.
    /// This is critical for sensitive operations like wallet methods.
    #[serde(default)]
    pub requires_auth: bool,
}

impl RpcMethodDescriptor {
    /// Creates a new RPC method descriptor.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            requires_auth: false,
        }
    }

    /// Creates a new RPC method descriptor that requires authentication.
    ///
    /// # Security
    /// Use this for sensitive operations like wallet methods, private key export,
    /// transaction signing, etc. These methods will be rejected if no authentication
    /// is configured on the RPC server.
    pub fn new_protected(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            requires_auth: true,
        }
    }

    /// Returns whether this method requires authentication.
    pub fn requires_auth(&self) -> bool {
        self.requires_auth
    }
}
