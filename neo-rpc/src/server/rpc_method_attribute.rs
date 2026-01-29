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
    /// Whether this method is marked as requiring authentication.
    /// Authentication is enforced globally when RPC basic auth is configured,
    /// matching Neo C# behavior. This flag is kept for metadata only.
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

    /// Creates a new RPC method descriptor marked as protected.
    ///
    /// # Security
    /// Use this for sensitive operations like wallet methods, private key export,
    /// and transaction signing. Enforcement is handled by global RPC auth.
    pub fn new_protected(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            requires_auth: true,
        }
    }

    /// Returns whether this method requires authentication.
    #[must_use] 
    pub const fn requires_auth(&self) -> bool {
        self.requires_auth
    }
}
