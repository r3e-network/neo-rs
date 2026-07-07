//! # neo-rpc::server::rpc_server_application_logs
//!
//! Application-log RPC endpoint handlers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `lookup`: Application-log lookup endpoint implementation.
//! - `request`: Typed JSON-RPC request parsing helpers.
//! - `response`: Application-log response filtering helpers.
//! - `tests`: Module-local tests and regression coverage.

use crate::server::rpc_server::RpcHandler;

mod lookup;
mod request;
mod response;

#[cfg(test)]
#[path = "../../tests/server/handlers/rpc_server_application_logs.rs"]
mod tests;

/// RPC handler group for the `ApplicationLogs` plugin methods.
pub struct RpcServerApplicationLogs;

impl RpcServerApplicationLogs {
    /// Register `ApplicationLogs` RPC handlers.
    pub fn register_handlers() -> Vec<RpcHandler> {
        super::rpc_handlers![
            "getapplicationlog" => Self::get_application_log,
        ]
    }
}
