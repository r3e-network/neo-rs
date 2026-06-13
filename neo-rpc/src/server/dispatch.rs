//! RPC dispatch core: handler lookup + panic-safe invocation.
//!
//! Extracted from the legacy warp-based `routes/handlers.rs` so the
//! same dispatch logic is reused by both the warp glue (deprecated) and
//! the `jsonrpsee`-based server in `super::jsonrpsee_adapter`. Once the
//! `routes/` directory is deleted, this module is the single home for
//! the dispatch path.

use super::rpc_error::RpcError;
use super::rpc_server::{RPC_ERR_TOTAL, RPC_REQ_TOTAL, RpcHandler, RpcServer};
use super::rpc_server_settings::{RpcServerSettings, UnhandledExceptionPolicy};
use parking_lot::RwLock;
use std::collections::HashSet;
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Weak};
use tracing::error;

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

    let Some(handler) = lookup_rpc_handler(&server_arc, &method_key) else {
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
    let policy = RpcServerSettings::current().exception_policy();
    let callback = handler.callback();
    let call_result = panic::catch_unwind(AssertUnwindSafe(|| {
        let server_guard = server_arc.read();
        (callback)(&server_guard, params)
    }));

    match call_result {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(err)) => Err(RpcError::from(err)),
        Err(payload) => {
            error!(
                target: "neo::rpc",
                method,
                error = panic_message(&payload),
                "rpc handler panicked"
            );
            match policy {
                UnhandledExceptionPolicy::StopPlugin => {
                    let mut server = server_arc.write();
                    server.stop_rpc_server();
                }
                UnhandledExceptionPolicy::StopNode => std::process::exit(1),
                UnhandledExceptionPolicy::Terminate => std::process::abort(),
                UnhandledExceptionPolicy::Ignore
                | UnhandledExceptionPolicy::Log
                | UnhandledExceptionPolicy::Continue => {}
            }
            Err(RpcError::internal_server_error())
        }
    }
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "panic".to_string()
    }
}

/// Re-export the jsonrpsee context type so consumers can build modules
/// without depending on the inner adapter module.
pub use super::jsonrpsee_adapter::JsonRpseeContext as DispatchContext;
