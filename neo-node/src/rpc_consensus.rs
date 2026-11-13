//! RPC handlers for consensus control.

use crate::consensus::DbftConsensusController;
use neo_rpc::server::{RpcException, RpcHandler, RpcMethodDescriptor, RpcServer, ServerRpcError};
use serde_json::Value;
use std::sync::Arc;

pub struct RpcServerConsensus;

impl RpcServerConsensus {
    pub fn register_handlers() -> Vec<RpcHandler> {
        vec![Self::handler("startconsensus", Self::start_consensus)]
    }

    fn handler(
        name: &'static str,
        func: fn(&RpcServer, &[Value]) -> Result<Value, RpcException>,
    ) -> RpcHandler {
        RpcHandler::new(RpcMethodDescriptor::new_protected(name), Arc::new(func))
    }

    fn start_consensus(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        if !params.is_empty() {
            return Err(RpcException::from(
                ServerRpcError::invalid_params().with_data("startconsensus expects no parameters"),
            ));
        }

        let controller = server
            .system()
            .get_service::<DbftConsensusController>()
            .map_err(|error| {
                RpcException::from(
                    ServerRpcError::internal_server_error().with_data(error.to_string()),
                )
            })?
            .ok_or_else(|| {
                RpcException::from(
                    ServerRpcError::invalid_params().with_data("Consensus not enabled"),
                )
            })?;

        let wallet = server
            .wallet()
            .ok_or_else(|| RpcException::from(ServerRpcError::no_opened_wallet()))?;

        if controller.is_running() {
            return Ok(Value::Bool(true));
        }

        Ok(Value::Bool(controller.start_with_wallet(wallet)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_the_real_consensus_control_method() {
        let handlers = RpcServerConsensus::register_handlers();
        assert_eq!(handlers.len(), 1);
        assert_eq!(handlers[0].descriptor().name, "startconsensus");
    }
}
