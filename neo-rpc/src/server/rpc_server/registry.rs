//! RPC handler registration and transport method projection.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLockReadGuard;

use super::{RpcHandler, RpcServer};

impl RpcServer {
    /// Register a single RPC handler.
    pub fn register_method(&mut self, handler: RpcHandler) {
        let key = handler.descriptor().name.to_ascii_lowercase();
        self.handler_lookup.write().insert(key, Arc::new(handler));
    }

    /// Register multiple RPC handlers.
    pub fn register_handlers(&mut self, handlers: Vec<RpcHandler>) {
        for handler in handlers {
            self.register_method(handler);
        }
    }

    pub(crate) fn handlers_guard(&self) -> RwLockReadGuard<'_, HashMap<String, Arc<RpcHandler>>> {
        self.handler_lookup.read()
    }

    /// Collects the sorted, deduplicated names of the public (non-auth)
    /// handlers directly from `&self`, taking only the inner handler-map lock.
    ///
    /// Used both by `crate::server::jsonrpsee_adapter` (after acquiring an
    /// outer read lock) and by `RpcServer::start_rpc_server` (which already
    /// holds the outer write lock and therefore cannot acquire the outer read
    /// lock).
    pub fn public_method_names(&self) -> Vec<String> {
        self.method_names(false)
    }

    /// Collects the sorted, deduplicated names of handlers exposed by the
    /// configured transport. Protected methods are only exposed when complete
    /// RPC credentials are configured; the transport middleware still enforces
    /// Basic auth before those handlers execute.
    pub fn transport_method_names(&self) -> Vec<String> {
        self.method_names(self.rpc_auth_configured())
    }

    fn method_names(&self, include_protected: bool) -> Vec<String> {
        let handlers = self.handlers_guard();
        let mut methods = handlers
            .values()
            .filter(|handler| include_protected || !handler.descriptor().requires_auth())
            .map(|handler| handler.descriptor().name.clone())
            .collect::<Vec<_>>();
        methods.sort_unstable();
        methods.dedup();
        methods
    }
}
