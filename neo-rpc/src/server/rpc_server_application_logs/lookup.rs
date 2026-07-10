//! Application-log lookup endpoint implementation.
//!
//! This module owns live ApplicationLogs service access for
//! `getapplicationlog`; request parsing and response filtering stay in the
//! sibling request/response modules.

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;
use crate::server::rpc_server::RpcServer;
use serde_json::Value;

use super::RpcServerApplicationLogs;
use super::request::ApplicationLogRequest;

impl RpcServerApplicationLogs {
    pub(super) fn get_application_log(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let request = ApplicationLogRequest::parse(params)?;
        let service = server
            .system()
            .application_logs_service()
            .ok_or_else(|| internal_error("ApplicationLogs service not available"))?;

        let raw = service
            .get_block_log(&request.hash)
            .or_else(|| service.get_transaction_log(&request.hash))
            .ok_or_else(|| {
                RpcException::from(
                    RpcError::invalid_params().with_data("Unknown transaction/blockhash"),
                )
            })?;

        Ok(super::response::apply_trigger_filter(
            raw,
            request.trigger_filter.as_deref(),
        ))
    }
}
