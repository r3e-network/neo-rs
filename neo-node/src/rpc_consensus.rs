//! RPC handlers for consensus control.
//!
//! TODO: Re-enable when consensus module is implemented

// use crate::consensus::DbftConsensusController;
use neo_rpc::server::{RpcException, RpcHandler, RpcMethodDescriptor, RpcServer, ServerRpcError};
use serde_json::Value;
use std::sync::Arc;

pub struct RpcServerConsensus;

impl RpcServerConsensus {
    pub fn register_handlers() -> Vec<RpcHandler> {
        // TODO: Re-enable when consensus module is available
        // vec![Self::handler("startconsensus", Self::start_consensus)]
        vec![]
    }

    #[allow(dead_code)]
    fn handler(
        name: &'static str,
        func: fn(&RpcServer, &[Value]) -> Result<Value, RpcException>,
    ) -> RpcHandler {
        RpcHandler::new(RpcMethodDescriptor::new(name), Arc::new(func))
    }

    #[allow(dead_code)]
    fn start_consensus(_server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
        // TODO: Re-enable when DbftConsensusController is available
        Err(RpcException::from(
            ServerRpcError::internal_server_error().with_data("Consensus not yet implemented"),
        ))

        // Original implementation (commented out):
        // if !params.is_empty() {
        //     return Err(RpcException::from(
        //         ServerRpcError::invalid_params().with_data("startconsensus expects no parameters"),
        //     ));
        // }
        //
        // let controller = server
        //     .system()
        //     .get_service::<DbftConsensusController>()
        //     .map_err(|err| {
        //         RpcException::from(
        //             ServerRpcError::internal_server_error().with_data(err.to_string()),
        //         )
        //     })?
        //     .ok_or_else(|| {
        //         RpcException::from(
        //             ServerRpcError::invalid_params().with_data("Consensus not enabled"),
        //         )
        //     })?;
        //
        // let wallet = server
        //     .wallet()
        //     .ok_or_else(|| RpcException::from(ServerRpcError::no_opened_wallet()))?;
        //
        // if controller.is_running() {
        //     return Ok(Value::Bool(true));
        // }
        //
        // Ok(Value::Bool(controller.start_with_wallet(wallet)))
    }
}
