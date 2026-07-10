//! RPC callback and handler descriptor bindings.
//!
//! This module owns the typed wrapper between JSON-RPC method descriptors and
//! the handler callbacks registered by each endpoint group.

use serde_json::Value;

use crate::server::rpc_exception::RpcException;
use crate::server::rpc_method_attribute::RpcMethodDescriptor;

use super::RpcServer;

/// Function-pointer signature used by registered RPC handlers.
///
/// RPC endpoint implementations are stateless function items. Keeping their
/// concrete function pointer avoids a heap allocation and virtual call on
/// every request while the descriptor still permits runtime method names.
pub type RpcCallback = fn(&RpcServer, &[Value]) -> Result<Value, RpcException>;

/// Registered RPC method descriptor and callback.
pub struct RpcHandler {
    descriptor: RpcMethodDescriptor,
    callback: RpcCallback,
}

impl RpcHandler {
    /// Create an RPC handler from a method descriptor and callback.
    pub const fn new(descriptor: RpcMethodDescriptor, callback: RpcCallback) -> Self {
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

    /// Return this handler's function pointer.
    #[must_use]
    pub const fn callback(&self) -> RpcCallback {
        self.callback
    }

    /// Invoke this handler without allocating or cloning callback state.
    pub fn call(&self, server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        (self.callback)(server, params)
    }
}

pub(crate) fn rpc_handler(
    name: &'static str,
    func: fn(&RpcServer, &[Value]) -> Result<Value, RpcException>,
) -> RpcHandler {
    RpcHandler::new(RpcMethodDescriptor::new(name), func)
}

pub(crate) fn protected_rpc_handler(
    name: &'static str,
    func: fn(&RpcServer, &[Value]) -> Result<Value, RpcException>,
) -> RpcHandler {
    RpcHandler::new(RpcMethodDescriptor::new_protected(name), func)
}
