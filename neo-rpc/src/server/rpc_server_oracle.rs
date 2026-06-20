use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::{expect_base64_param, invalid_params};
use crate::server::rpc_server::{RpcHandler, RpcServer};
use neo_crypto::{ECCurve, ECPoint};
use neo_oracle_service::{OracleService, OracleServiceError};
use serde_json::{Value, json};
use std::sync::Arc;

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
        let oracle_pubkey_bytes = expect_base64_param(params, 0, "submitoracleresponse")?;
        let request_id =
            crate::server::rpc_helpers::expect_u64_param(params, 1, "submitoracleresponse")?;
        let tx_sign = expect_base64_param(params, 2, "submitoracleresponse")?;
        let msg_sign = expect_base64_param(params, 3, "submitoracleresponse")?;

        let oracle_pub = ECPoint::from_bytes_with_curve(ECCurve::Secp256r1, &oracle_pubkey_bytes)
            .map_err(|_| invalid_params("Invalid oracle public key"))?;

        let service = oracle_service(server)?;
        service
            .submit_oracle_response(oracle_pub, request_id, tx_sign, msg_sign)
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
mod tests {
    use super::*;

    #[test]
    fn map_oracle_error_includes_signature_message() {
        let err = OracleServiceError::InvalidSignature("bad signature".to_string());
        let rpc = map_oracle_error(err);
        assert_eq!(rpc.code(), RpcError::invalid_signature().code());
        assert_eq!(rpc.data(), Some("bad signature"));
    }

    #[test]
    fn map_oracle_error_includes_not_designated_message() {
        let err = OracleServiceError::NotDesignated("not oracle".to_string());
        let rpc = map_oracle_error(err);
        assert_eq!(rpc.code(), RpcError::oracle_not_designated_node().code());
        assert_eq!(rpc.data(), Some("not oracle"));
    }
}
