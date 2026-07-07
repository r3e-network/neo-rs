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
//! - `request`: Typed JSON-RPC request parsing helpers.
//! - `response`: Application-log response filtering helpers.
//! - `tests`: Module-local tests and regression coverage.

use crate::application_logs::ApplicationLogsService;
use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;
use crate::server::rpc_server::{RpcHandler, RpcServer};
use serde_json::Value;

mod request;
mod response;

use self::request::ApplicationLogRequest;

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

    fn get_application_log(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let request = ApplicationLogRequest::parse(params)?;
        let service = server
            .system()
            .get_service::<ApplicationLogsService>()
            .ok_or_else(|| internal_error("ApplicationLogs service not available"))?;

        let raw = service
            .get_block_log(&request.hash)
            .or_else(|| service.get_transaction_log(&request.hash))
            .ok_or_else(|| {
                RpcException::from(
                    RpcError::invalid_params().with_data("Unknown transaction/blockhash"),
                )
            })?;

        Ok(response::apply_trigger_filter(
            raw,
            request.trigger_filter.as_deref(),
        ))
    }
}
