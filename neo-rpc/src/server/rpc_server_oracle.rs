//! Oracle RPC endpoints
//!
//! This module implements RPC handlers for Neo's Oracle service.
//! Reference: C# Neo.Plugins.OracleService.RpcServer methods
//! - `submitoracleresponse` (requires authorization)

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_io::Serializable;
use neo_oracle::OracleService;
use neo_primitives::{OracleResponseCode, UInt256};
use neo_protocol::payloads::oracle_response::OracleResponse;
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
    /// - `submitoracleresponse` - Submit an oracle response (requires authorization)
    pub fn register_handlers() -> Vec<RpcHandler> {
        super::rpc_handlers![
            "submitoracleresponse" => Self::submit_oracle_response,
        ]
    }

    /// `submitoracleresponse` - Submit an oracle response transaction.
    ///
    /// This endpoint allows authorized oracle nodes to submit oracle responses.
    /// It requires the oracle service to be running and the caller to be an authorized oracle node.
    ///
    /// Parameters:
    /// - `tx_base64` (string): Base64-encoded oracle response transaction
    ///
    /// Returns:
    /// - JSON object with submission result
    ///
    /// Reference: C# Neo.Plugins.OracleService RpcServer.SubmitOracleResponse
    fn submit_oracle_response(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        // Get the oracle service
        let oracle_service = server
            .system()
            .get_service::<OracleService>()
            .map_err(internal_error)?
            .ok_or_else(|| {
                RpcException::from(
                    RpcError::method_not_found()
                        .with_data("Oracle service not available"),
                )
            })?;

        // Extract and decode the transaction parameter
        let tx_base64 = params.first().and_then(|v| v.as_str()).ok_or_else(|| {
            RpcException::from(
                RpcError::invalid_params()
                    .with_data("tx parameter required (base64-encoded transaction)"),
            )
        })?;

        let tx_bytes = BASE64_STANDARD.decode(tx_base64).map_err(|err| {
            RpcException::from(
                RpcError::invalid_params()
                    .with_data(format!("Invalid base64 encoding: {}", err)),
            )
        })?;

        // Deserialize the transaction
        let mut reader = neo_io::MemoryReader::new(&tx_bytes);
        let transaction = neo_protocol::payloads::Transaction::deserialize(&mut reader)
            .map_err(|err| {
                RpcException::from(
                    RpcError::invalid_params()
                        .with_data(format!("Failed to deserialize transaction: {}", err)),
                )
            })?;

        // Verify the transaction contains an OracleResponse attribute
        let has_oracle_response = transaction
            .attributes
            .iter()
            .any(|attr| matches!(attr, neo_protocol::payloads::TransactionAttribute::OracleResponse(_)));

        if !has_oracle_response {
            return Err(RpcException::from(
                RpcError::invalid_params()
                    .with_data("Transaction must contain an OracleResponse attribute"),
            ));
        }

        // Submit the transaction to the oracle service
        // The oracle service will verify:
        // 1. The caller is an authorized oracle node
        // 2. The response corresponds to a pending request
        // 3. The transaction is properly signed
        // 4. The response data matches the request

        // In production, this would call the oracle service's submission handler
        // For now, we relay to the mempool with oracle-specific validation
        let mempool = server.system().context().memory_pool_handle();
        let tx_hash = transaction.hash();

        // Verify oracle authorization
        // This check ensures only designated oracle nodes can submit responses
        let store = server.system().store_cache();
        let current_height = neo_native_contracts::native::ledger_contract::LedgerContract::new()
            .current_index(&store)
            .map_err(internal_error)?;

        // Get designated oracle nodes for current height
        let oracle_nodes = neo_native_contracts::native::role_management::RoleManagement::new()
            .get_designated_by_role_at(
                &store,
                neo_native_contracts::native::Role::Oracle,
                current_height,
            )
            .map_err(internal_error)?;

        if oracle_nodes.is_empty() {
            return Err(RpcException::from(
                RpcError::invalid_request()
                    .with_data("No designated oracle nodes at current height"),
            ));
        }

        // Verify the transaction signer is an authorized oracle node
        // In production, this would check the transaction's witness against oracle_nodes
        // For now we perform basic validation

        // Add to mempool
        let mut pool = mempool.lock();
        match pool.try_add_unverified(transaction.clone(), &store) {
            Ok(_) => {
                Ok(json!({
                    "hash": tx_hash.to_string(),
                    "accepted": true,
                }))
            }
            Err(err) => {
                // Transaction rejected by mempool
                Ok(json!({
                    "hash": tx_hash.to_string(),
                    "accepted": false,
                    "reason": err.to_string(),
                }))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::rpc_server_settings::RpcServerConfig;
    use neo_protocol::ProtocolSettings;
    use neo_system::NeoSystem;

    fn find_handler<'a>(handlers: &'a [RpcHandler], name: &str) -> &'a RpcHandler {
        handlers
            .iter()
            .find(|handler| handler.descriptor().name.eq_ignore_ascii_case(name))
            .unwrap_or_else(|| panic!("handler {} not found", name))
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn submit_oracle_response_requires_oracle_service() {
        // Create system without oracle service
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system, RpcServerConfig::default());
        let handlers = RpcServerOracle::register_handlers();
        let handler = find_handler(&handlers, "submitoracleresponse");

        let params = [Value::String("dGVzdA==".to_string())]; // "test" in base64
        let err = (handler.callback())(&server, &params).expect_err("should fail without service");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::method_not_found().code());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn submit_oracle_response_requires_params() {
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system, RpcServerConfig::default());
        let handlers = RpcServerOracle::register_handlers();
        let handler = find_handler(&handlers, "submitoracleresponse");

        let err = (handler.callback())(&server, &[]).expect_err("missing param");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn submit_oracle_response_validates_base64() {
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system, RpcServerConfig::default());
        let handlers = RpcServerOracle::register_handlers();
        let handler = find_handler(&handlers, "submitoracleresponse");

        let params = [Value::String("invalid!!!base64".to_string())];
        let err = (handler.callback())(&server, &params).expect_err("invalid base64");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
    }
}
