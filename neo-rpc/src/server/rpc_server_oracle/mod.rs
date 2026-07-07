//! # neo-rpc::server::rpc_server_oracle
//!
//! Oracle RPC endpoint handlers.
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
//! - `tests`: Module-local tests and regression coverage.

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::invalid_params;
use crate::server::rpc_server::{RpcHandler, RpcServer};
use neo_oracle_service::{OracleService, OracleServiceError};
use serde_json::{Value, json};
use std::sync::Arc;

mod request;

use self::request::SubmitOracleResponseRequest;

/// RPC handler group for Oracle service methods.
pub struct RpcServerOracle;

impl RpcServerOracle {
    /// Register Oracle RPC handlers.
    pub fn register_handlers() -> Vec<RpcHandler> {
        super::rpc_handlers![
            "submitoracleresponse" => Self::submit_oracle_response,
        ]
    }

    fn submit_oracle_response(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let request = SubmitOracleResponseRequest::parse(params)?;
        let service = oracle_service(server)?;
        service
            .submit_oracle_response(
                request.oracle_pubkey,
                request.request_id,
                request.tx_signature,
                request.message_signature,
            )
            .map_err(map_oracle_error)?;

        Ok(json!({}))
    }
}

fn oracle_service(server: &RpcServer) -> Result<Arc<OracleService>, RpcException> {
    server
        .system()
        .get_service::<OracleService>()
        .ok_or_else(|| RpcException::from(RpcError::oracle_disabled()))
}

fn map_oracle_error(err: OracleServiceError) -> RpcException {
    match err {
        OracleServiceError::Disabled => RpcException::from(RpcError::oracle_disabled()),
        OracleServiceError::RequestFinished | OracleServiceError::DuplicateRequest => {
            RpcException::from(RpcError::oracle_request_finished())
        }
        OracleServiceError::RequestNotFound
        | OracleServiceError::RequestTransactionNotFound
        | OracleServiceError::BuildFailed(_) => {
            RpcException::from(RpcError::oracle_request_not_found())
        }
        OracleServiceError::NotDesignated(msg) => {
            RpcException::from(RpcError::oracle_not_designated_node().with_data(msg))
        }
        OracleServiceError::InvalidSignature(msg) => {
            RpcException::from(RpcError::invalid_signature().with_data(msg))
        }
        OracleServiceError::InvalidOraclePublicKey => invalid_params("Invalid oracle public key"),
        OracleServiceError::Processing(msg) => {
            RpcException::from(RpcError::internal_server_error().with_data(msg))
        }
        OracleServiceError::UrlBlocked => invalid_params("URL blocked by security policy"),
    }
}

#[cfg(test)]
#[path = "../../tests/server/handlers/rpc_server_oracle.rs"]
mod tests;
