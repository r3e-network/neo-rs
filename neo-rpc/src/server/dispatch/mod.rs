//! # neo-rpc::server::dispatch
//!
//! RPC method dispatch, registration, and handler lookup helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `panic_policy`: RPC handler panic capture and exception-policy handling.

mod panic_policy;

use super::rpc_error::RpcError;
use super::rpc_remote_ledger::should_proxy_remote_ledger_method;
use super::rpc_server::{RpcHandler, RpcServer};
use parking_lot::RwLock;
use std::collections::HashSet;
use std::sync::{Arc, Weak};

pub struct Dispatch;

impl Dispatch {
    /// Look up a registered RPC handler by method name (case-insensitive).
    ///
    /// Returns `Err(RpcError::access_denied())` for disabled methods,
    /// `Err(RpcError::internal_server_error())` if the server has been
    /// dropped, and `Err(RpcError::method_not_found())` for unknown methods.
    pub(crate) fn resolve_rpc_handler(
        server: &Weak<RwLock<RpcServer>>,
        disabled: &HashSet<String>,
        method: &str,
    ) -> Result<(Arc<RwLock<RpcServer>>, Arc<RpcHandler>), RpcError> {
        let method_key = method.to_ascii_lowercase();
        if disabled.contains(&method_key) {
            return Err(RpcError::access_denied());
        }

        let Some(server_arc) = server.upgrade() else {
            return Err(RpcError::internal_server_error());
        };

        let Some(handler) = Dispatch::lookup_rpc_handler(&server_arc, &method_key) else {
            return Err(RpcError::method_not_found().with_data(method));
        };

        Ok((server_arc, handler))
    }

    /// Look up a handler in the server's method registry.
    pub(crate) fn lookup_rpc_handler(
        server_arc: &Arc<RwLock<RpcServer>>,
        method_key: &str,
    ) -> Option<Arc<RpcHandler>> {
        let server_guard = server_arc.read();
        let guard = server_guard.handlers_guard();
        guard.get(method_key).cloned()
    }

    /// Invoke a registered handler, catching panics and applying the
    /// configured `UnhandledExceptionPolicy`.
    pub(crate) fn invoke_rpc_handler(
        server_arc: &Arc<RwLock<RpcServer>>,
        handler: Arc<RpcHandler>,
        method: &str,
        params: &[serde_json::Value],
    ) -> Result<serde_json::Value, RpcError> {
        let policy = panic_policy::current_policy();
        let canonical_method = handler.descriptor().name.clone();
        let remote_ledger = {
            let server_guard = server_arc.read();
            server_guard.check_rate_limit(&canonical_method)?;
            if should_proxy_remote_ledger_method(&canonical_method) {
                server_guard.remote_ledger_rpc().cloned()
            } else {
                None
            }
        };
        if let Some(remote) = remote_ledger {
            return remote.call(&canonical_method, params);
        }
        panic_policy::invoke_local_handler(server_arc, handler.as_ref(), method, params, policy)
    }
}

#[cfg(test)]
#[path = "../../tests/server/core/dispatch.rs"]
mod tests;
