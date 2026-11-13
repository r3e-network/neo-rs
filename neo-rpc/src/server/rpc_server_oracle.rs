//! Oracle RPC endpoints
//!
//! Implements the oracle-node-to-oracle-node `submitoracleresponse` endpoint,
//! mirroring C# `Neo.Plugins.OracleService.RpcServer.SubmitOracleResponse`.
//!
//! Security model (fail-closed):
//! - Submissions are only accepted through the node's running
//!   [`OracleService`], never by injecting a transaction into the mempool
//!   directly from RPC input.
//! - The service enforces the full authorization chain: the submitting public
//!   key must belong to the designated oracle node set, the request must exist
//!   and not be finished, and both the message and transaction signatures must
//!   verify before the response transaction is relayed.
//! - When the service is missing or not running, submission is rejected with
//!   an explicit error instead of being silently accepted.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_crypto::ECPoint;
use neo_system::oracle_service::{OracleService, OracleServiceError};
use serde_json::{Value, json};

use super::rpc_error::RpcError;
use super::rpc_exception::RpcException;
use super::rpc_helpers::internal_error;
use super::rpc_server::{RpcHandler, RpcServer};

/// Oracle RPC service implementation.
pub struct RpcServerOracle;

impl RpcServerOracle {
    /// Registers all Oracle RPC handlers.
    ///
    /// Available methods:
    /// - `submitoracleresponse` - Submit an oracle response signature
    ///   (requires the oracle service and a designated oracle node)
    pub fn register_handlers() -> Vec<RpcHandler> {
        super::rpc_handlers![
            "submitoracleresponse" => Self::submit_oracle_response,
        ]
    }

    /// `submitoracleresponse` - Records an oracle response signature.
    ///
    /// This endpoint is used by peer oracle nodes to aggregate response
    /// signatures; the wire format matches the payload produced by
    /// `OracleService::send_response_signature`.
    ///
    /// Parameters:
    /// - `oracle_pub` (string, base64): the submitting oracle node's public key
    /// - `request_id` (number): the oracle request id
    /// - `tx_sign` (string, base64): the signature of the response transaction
    /// - `msg_sign` (string, base64): the signature of the submission message
    ///
    /// Returns:
    /// - JSON object `{ "accepted": true }` once the signature is recorded
    ///
    /// Reference: C# Neo.Plugins.OracleService RpcServer.SubmitOracleResponse
    fn submit_oracle_response(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        const METHOD: &str = "submitoracleresponse";
        const PARAM_COUNT: usize = 4;

        // Authorization gate, fail-closed: submissions must flow through the
        // oracle service, which re-validates designation, request state and
        // signatures. A node without the oracle service never accepts one.
        let service = server
            .system()
            .get_service::<OracleService>()
            .map_err(internal_error)?
            .ok_or_else(|| {
                RpcException::from(
                    RpcError::method_not_found()
                        .with_data("Oracle service is not enabled on this node"),
                )
            })?;

        if params.len() < PARAM_COUNT {
            return Err(RpcException::from(
                RpcError::invalid_params().with_data(format!(
                    "{METHOD} expects {PARAM_COUNT} parameters: oracle public key (base64), request id, transaction signature (base64), message signature (base64)"
                )),
            ));
        }

        let oracle_pub_bytes = decode_base64_param(params, 0, METHOD)?;
        let oracle_pub = ECPoint::from_bytes(&oracle_pub_bytes).map_err(|err| {
            RpcException::from(
                RpcError::invalid_params().with_data(format!("Invalid oracle public key: {err}")),
            )
        })?;

        let request_id = params[1].as_u64().ok_or_else(|| {
            RpcException::from(
                RpcError::invalid_params()
                    .with_data(format!("{METHOD} expects a numeric request id")),
            )
        })?;

        let tx_sign = decode_base64_param(params, 2, METHOD)?;
        let msg_sign = decode_base64_param(params, 3, METHOD)?;

        // Delegate to the real oracle service path. The service enforces the
        // authorization chain (designated oracle node, pending request,
        // signature verification) before aggregating and relaying anything.
        service
            .submit_oracle_response(oracle_pub, request_id, tx_sign, msg_sign)
            .map_err(|err| RpcException::from(rpc_error_for_service_error(err)))?;

        Ok(json!({ "accepted": true }))
    }
}

/// Decodes the base64 string at `params[index]` into raw bytes.
fn decode_base64_param(
    params: &[Value],
    index: usize,
    method: &str,
) -> Result<Vec<u8>, RpcException> {
    let encoded = params[index].as_str().ok_or_else(|| {
        RpcException::from(RpcError::invalid_params().with_data(format!(
            "{method} parameter {index} must be a base64 string"
        )))
    })?;
    BASE64_STANDARD.decode(encoded).map_err(|err| {
        RpcException::from(RpcError::invalid_params().with_data(format!(
            "{method} parameter {index} is not valid base64: {err}"
        )))
    })
}

/// Maps an oracle service rejection to an explicit RPC error.
///
/// Every variant surfaces as a visible error; there is no path on which a
/// rejected submission is treated as accepted.
fn rpc_error_for_service_error(err: OracleServiceError) -> RpcError {
    match err {
        OracleServiceError::Disabled => RpcError::method_not_found()
            .with_data("Oracle service is not running; submission rejected"),
        OracleServiceError::NotDesignated(_) => {
            RpcError::invalid_request().with_data(err.to_string())
        }
        OracleServiceError::InvalidSignature(_) => {
            RpcError::invalid_request().with_data(err.to_string())
        }
        OracleServiceError::RequestNotFound | OracleServiceError::RequestFinished => {
            RpcError::invalid_params().with_data(err.to_string())
        }
        other => RpcError::invalid_params().with_data(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::rpc_server_settings::RpcServerConfig;
    use neo_protocol::ProtocolSettings;
    use neo_system::NeoSystem;
    use neo_system::oracle_service::OracleServiceSettings;
    use std::sync::Arc;

    fn find_handler<'a>(handlers: &'a [RpcHandler], name: &str) -> &'a RpcHandler {
        handlers
            .iter()
            .find(|handler| handler.descriptor().name.eq_ignore_ascii_case(name))
            .unwrap_or_else(|| panic!("handler {} not found", name))
    }

    /// Builds an RPC server whose system has a registered-but-unstarted oracle
    /// service (an empty store has no designated oracles, so `start` refuses
    /// and the service stays disabled — the strongest fail-closed baseline).
    fn server_with_registered_oracle_service() -> RpcServer {
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let settings = OracleServiceSettings {
            network: system.settings().network,
            ..Default::default()
        };
        let service = OracleService::new(settings, system.clone()).expect("oracle service");
        let service = Arc::new(service);
        service.set_self_ref();
        system
            .add_service::<OracleService, _>(Arc::clone(&service))
            .expect("register oracle service");
        RpcServer::new(system, RpcServerConfig::default())
    }

    /// Valid-shaped parameters for the 4-argument wire format. The public
    /// key is the compressed secp256r1 generator point, so it parses as a real
    /// on-curve point (unlike arbitrary test bytes, which `ECPoint::from_bytes`
    /// must reject).
    fn submission_params() -> [Value; 4] {
        let oracle_pub = [
            0x02u8, 0x6b, 0x17, 0xd1, 0xf2, 0xe1, 0x2c, 0x42, 0x47, 0xf8, 0xbc, 0xe6, 0xe5, 0x63,
            0xa4, 0x40, 0xf2, 0x77, 0x03, 0x7d, 0x81, 0x2d, 0xeb, 0x33, 0xa0, 0xf4, 0xa1, 0x39,
            0x45, 0xd8, 0x98, 0xc2, 0x96,
        ];
        [
            Value::String(BASE64_STANDARD.encode(oracle_pub)),
            Value::Number(serde_json::Number::from(1u64)),
            Value::String(BASE64_STANDARD.encode([0x33u8; 64])),
            Value::String(BASE64_STANDARD.encode([0x44u8; 64])),
        ]
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn submit_oracle_response_fails_closed_without_service() {
        // No oracle service registered: the method must reject, never accept.
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system, RpcServerConfig::default());
        let handlers = RpcServerOracle::register_handlers();
        let handler = find_handler(&handlers, "submitoracleresponse");

        let err = (handler.callback())(&server, &submission_params())
            .expect_err("must reject without oracle service");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::method_not_found().code());
        assert!(
            rpc_error.data().unwrap_or_default().contains("not enabled"),
            "rejection must state why the submission was refused, got: {:?}",
            rpc_error.data()
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn submit_oracle_response_reaches_service_gate_and_fails_closed() {
        // The full call chain executes (handler -> service registry ->
        // OracleService shell -> engine authorization gate) and the engine's
        // own disabled gate rejects: an unstarted service must never accept a
        // submission.
        let server = server_with_registered_oracle_service();
        let handlers = RpcServerOracle::register_handlers();
        let handler = find_handler(&handlers, "submitoracleresponse");

        let err = (handler.callback())(&server, &submission_params())
            .expect_err("unstarted oracle service must reject submissions");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::method_not_found().code());
        // The message is produced by the engine gate, proving the submission
        // reached the oracle service instead of a local shortcut.
        assert!(
            rpc_error.data().unwrap_or_default().contains("not running"),
            "engine disabled gate must be the rejection source, got: {:?}",
            rpc_error.data()
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn submit_oracle_response_rejects_missing_params() {
        let server = server_with_registered_oracle_service();
        let handlers = RpcServerOracle::register_handlers();
        let handler = find_handler(&handlers, "submitoracleresponse");

        let err = (handler.callback())(&server, &[]).expect_err("missing params");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn submit_oracle_response_rejects_invalid_public_key() {
        let server = server_with_registered_oracle_service();
        let handlers = RpcServerOracle::register_handlers();
        let handler = find_handler(&handlers, "submitoracleresponse");

        let mut params = submission_params();
        // 32 arbitrary bytes are not a valid secp256r1/r1 point.
        params[0] = Value::String(BASE64_STANDARD.encode([0x21u8; 32]));
        let err = (handler.callback())(&server, &params).expect_err("invalid public key");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
        assert!(
            rpc_error.data().unwrap_or_default().contains("public key"),
            "rejection must identify the invalid parameter, got: {:?}",
            rpc_error.data()
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn submit_oracle_response_rejects_invalid_base64() {
        let server = server_with_registered_oracle_service();
        let handlers = RpcServerOracle::register_handlers();
        let handler = find_handler(&handlers, "submitoracleresponse");

        let mut params = submission_params();
        params[3] = Value::String("invalid!!!base64".to_string());
        let err = (handler.callback())(&server, &params).expect_err("invalid base64");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn submit_oracle_response_rejects_non_numeric_request_id() {
        let server = server_with_registered_oracle_service();
        let handlers = RpcServerOracle::register_handlers();
        let handler = find_handler(&handlers, "submitoracleresponse");

        let mut params = submission_params();
        params[1] = Value::String("not-a-number".to_string());
        let err = (handler.callback())(&server, &params).expect_err("invalid request id");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
    }
}
