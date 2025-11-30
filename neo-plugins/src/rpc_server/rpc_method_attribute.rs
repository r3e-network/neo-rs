// Copyright (C) 2015-2025 The Neo Project.
//
// Rust equivalent of `Neo.Plugins.RpcServer.RpcMethodAttribute`. The original
// attribute marks reflection-based RPC methods; in Rust we represent this as a
// lightweight descriptor that can annotate registered handlers.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct RpcMethodDescriptor {
    pub name: String,
}

impl RpcMethodDescriptor {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}
