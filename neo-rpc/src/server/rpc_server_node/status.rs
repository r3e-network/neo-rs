//! Peer status endpoint handlers.

use neo_network::handle::LocalNodeInfo;
use serde_json::{Value, json};

use super::{RpcServerNode, request::NoParamsRequest};
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_server::RpcServer;

impl RpcServerNode {
    pub(super) fn get_connection_count(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        NoParamsRequest::parse(params, "getconnectioncount")?;
        Self::with_local_node(server, |node| json!(node.connected_peers_count()))
    }

    pub(super) fn get_peers(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        NoParamsRequest::parse(params, "getpeers")?;
        // C# `RpcServer.GetPeers` (RpcServer.Node.cs): three arrays of
        // `{"address": ..., "port": ...}` objects.
        //
        // - `unconnected`: C# serves `LocalNode.GetUnconnectedPeers()`.
        //   The reth-style network service keeps no unconnected address
        //   book (no `addr`-message peer discovery yet), so the list is
        //   served empty rather than invented.
        // - `bad`: always an empty array in C# v3.9.1 (no bad-peer book).
        // - `connected`: C# serves `Remote.Address` + `ListenerTcpPort`
        //   per remote node. The handle-side tracker folds the service's
        //   `PeerConnected` events, which carry exactly that pair:
        //   outbound dials publish the dialed endpoint (the peer's
        //   listener); inbound accepts publish `(remote_ip, 0)` - the
        //   C# unknown-listener form - and the per-peer service
        //   re-publishes the upgraded
        //   `(remote_ip, advertised_listener_port)` endpoint once the
        //   version handshake captures the peer's `TcpServer`
        //   capability (see
        //   `neo_runtime::NetworkEvent::PeerConnected`). Peers whose
        //   address never became known at the handle seam are counted
        //   by `getconnectioncount` but omitted here, since fabricating
        //   an address would corrupt the shape.
        Self::with_local_node(server, |node| {
            let connected: Vec<Value> = node
                .connected_peers()
                .iter()
                .filter_map(|peer| {
                    peer.address.map(|addr| {
                        json!({
                            "address": addr.ip().to_string(),
                            "port": addr.port()})
                    })
                })
                .collect();
            json!({
                "unconnected": Vec::<Value>::new(),
                "bad": Vec::<Value>::new(),
                "connected": connected})
        })
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
