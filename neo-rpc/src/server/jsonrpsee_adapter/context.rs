//! Shared context carried by jsonrpsee method callbacks.

use crate::server::rpc_server::RpcServer;
use parking_lot::RwLock;
use std::collections::HashSet;
use std::sync::{Arc, Weak};

/// Shared context used by the jsonrpsee RPC module.
#[derive(Clone)]
pub struct JsonRpseeContext {
    pub(super) server: Weak<RwLock<RpcServer>>,
    pub(super) disabled: Arc<HashSet<String>>,
}

impl JsonRpseeContext {
    /// Create a jsonrpsee module context with the live RPC server reference.
    #[must_use]
    pub fn new(server: Weak<RwLock<RpcServer>>, disabled: Arc<HashSet<String>>) -> Self {
        Self { server, disabled }
    }
}
