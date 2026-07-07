//! RPC callback and handler descriptor bindings.
//!
//! This module owns the typed wrapper between JSON-RPC method descriptors and
//! the handler callbacks registered by each endpoint group.

use std::sync::Arc;

use serde_json::Value;

use crate::server::rpc_exception::RpcException;
use crate::server::rpc_method_attribute::RpcMethodDescriptor;

use super::RpcServer;

/// Callback signature used by registered RPC handlers.
pub type RpcCallback =
    dyn Fn(&RpcServer, &[Value]) -> Result<Value, RpcException> + Send + Sync + 'static;

/// Registered RPC method descriptor and callback.
pub struct RpcHandler {
    descriptor: RpcMethodDescriptor,
    callback: Arc<RpcCallback>,
}

impl RpcHandler {
    /// Create an RPC handler from a method descriptor and callback.
    pub fn new(descriptor: RpcMethodDescriptor, callback: Arc<RpcCallback>) -> Self {
        Self {
            descriptor,
            callback,
        }
    }

    /// Return this handler's method descriptor.
    #[must_use]
    pub const fn descriptor(&self) -> &RpcMethodDescriptor {
        &self.descriptor
    }

    /// Return a clone of this handler's callback.
    #[must_use]
    pub fn callback(&self) -> Arc<RpcCallback> {
        Arc::clone(&self.callback)
    }
}

pub(crate) fn rpc_handler(
    name: &'static str,
    func: fn(&RpcServer, &[Value]) -> Result<Value, RpcException>,
) -> RpcHandler {
    RpcHandler::new(RpcMethodDescriptor::new(name), Arc::new(func))
}

pub(crate) fn protected_rpc_handler(
    name: &'static str,
    func: fn(&RpcServer, &[Value]) -> Result<Value, RpcException>,
) -> RpcHandler {
    RpcHandler::new(RpcMethodDescriptor::new_protected(name), Arc::new(func))
}
