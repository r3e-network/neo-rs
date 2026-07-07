//! Oracle response submission endpoint implementation.

use std::sync::Arc;

use neo_oracle_service::{OracleService, OracleServiceError};
use serde_json::Value;

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::invalid_params;
use crate::server::rpc_server::RpcServer;

use super::RpcServerOracle;
use super::request::SubmitOracleResponseRequest;
use super::response::submit_oracle_response_to_json;

impl RpcServerOracle {
    pub(super) fn submit_oracle_response(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
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

        Ok(submit_oracle_response_to_json())
    }
}

fn oracle_service(server: &RpcServer) -> Result<Arc<OracleService>, RpcException> {
    server
        .system()
        .get_service::<OracleService>()
        .ok_or_else(|| RpcException::from(RpcError::oracle_disabled()))
}

pub(super) fn map_oracle_error(err: OracleServiceError) -> RpcException {
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
