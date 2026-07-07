//! Transaction and block relay endpoint handlers.

use serde_json::Value;

use super::RpcServerNode;
use super::request::{RawTransactionRequest, SubmitBlockRequest};
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_relay;
use crate::server::rpc_server::RpcServer;

impl RpcServerNode {
    pub(super) fn send_raw_transaction(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let request = RawTransactionRequest::parse(params)?;
        let relay_result = rpc_relay::relay_transaction(server, request.transaction)?;
        rpc_relay::map_relay_result(relay_result)
    }

    pub(super) fn submit_block(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let request = SubmitBlockRequest::parse(params)?;
        let relay_result = rpc_relay::relay_block(server, request.block)?;
        rpc_relay::map_relay_result(relay_result)
    }
}
