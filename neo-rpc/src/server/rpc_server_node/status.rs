//! Peer status endpoint handlers.

use neo_network::LocalNodeInfo;
use serde_json::Value;

use super::{
    RpcServerNode,
    request::NoParamsRequest,
    response::{connection_count_to_json, peers_to_json},
};
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_server::RpcServer;

impl RpcServerNode {
    pub(super) fn get_connection_count(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        NoParamsRequest::parse(params, "getconnectioncount")?;
        Self::with_local_node(server, connection_count_to_json)
    }

    pub(super) fn get_peers(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        NoParamsRequest::parse(params, "getpeers")?;
        Self::with_local_node(server, peers_to_json)
    }

    pub(super) fn with_local_node<F>(server: &RpcServer, func: F) -> Result<Value, RpcException>
    where
        F: FnOnce(&LocalNodeInfo) -> Value,
    {
        let local = Self::fetch_local_node(server);
        Ok(func(&local))
    }

    fn fetch_local_node(server: &RpcServer) -> LocalNodeInfo {
        server.system().network().local_node_info()
    }
}
