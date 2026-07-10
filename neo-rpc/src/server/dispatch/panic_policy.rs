//! RPC handler panic capture and exception-policy handling.
//!
//! Dispatch decides which handler should run. This module owns the local
//! execution guard around that handler and applies the configured C#-compatible
//! unhandled-exception policy when a handler panics.

use parking_lot::RwLock;
use serde_json::Value;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use tracing::error;

use super::super::rpc_error::RpcError;
use super::super::rpc_server::{RpcHandler, RpcServer};
use super::super::rpc_server_settings::{RpcServerSettings, UnhandledExceptionPolicy};

/// Return the process-wide RPC unhandled-exception policy.
pub(super) fn current_policy() -> UnhandledExceptionPolicy {
    RpcServerSettings::current().exception_policy()
}

/// Invoke a local RPC handler while applying panic policy semantics.
pub(super) fn invoke_local_handler(
    server_arc: &Arc<RwLock<RpcServer>>,
    handler: &RpcHandler,
    method: &str,
    params: &[Value],
    policy: UnhandledExceptionPolicy,
) -> Result<Value, RpcError> {
    let call_result = panic::catch_unwind(AssertUnwindSafe(|| {
        let server_guard = server_arc.read();
        handler.call(&server_guard, params)
    }));

    match call_result {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(err)) => Err(RpcError::from(err)),
        Err(payload) => handle_handler_panic(server_arc, method, policy, payload),
    }
}

fn handle_handler_panic(
    server_arc: &Arc<RwLock<RpcServer>>,
    method: &str,
    policy: UnhandledExceptionPolicy,
    payload: Box<dyn std::any::Any + Send>,
) -> Result<Value, RpcError> {
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

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "panic".to_string()
    }
}
