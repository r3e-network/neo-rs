//! # neo-rpc::server::rpc_server_node
//!
//! Node and network RPC endpoint handlers.
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
//! - `version`: C#-compatible `getversion` response construction.

use crate::server::rpc_exception::RpcException;
use crate::server::rpc_relay;
use crate::server::rpc_server::{RpcHandler, RpcServer};
#[cfg(test)]
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_network::handle::LocalNodeInfo;
use serde_json::{Value, json};

mod request;
mod version;

use self::request::{RawTransactionRequest, SubmitBlockRequest};
#[cfg(test)]
use self::version::{
    LEDGER_PREFIX_CURRENT_BLOCK, POLICY_PREFIX_MAX_TRACEABLE_BLOCKS,
    POLICY_PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT, POLICY_PREFIX_MILLISECONDS_PER_BLOCK,
};

#[cfg(test)]
#[path = "../../tests/server/handlers/rpc_server_node.rs"]
mod tests;

/// RPC handler group for node status and relay methods.
pub struct RpcServerNode;

impl RpcServerNode {
    /// Register node RPC handlers.
    pub fn register_handlers() -> Vec<RpcHandler> {
        super::rpc_handlers![
            "getconnectioncount" => Self::get_connection_count,
            "getpeers" => Self::get_peers,
            "getversion" => Self::get_version,
            "sendrawtransaction" => Self::send_raw_transaction,
            "submitblock" => Self::submit_block,
        ]
    }

    fn get_connection_count(server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
        Self::with_local_node(server, |node| json!(node.connected_peers_count()))
    }

    fn get_peers(server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
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
        //   listener); inbound accepts publish `(remote_ip, 0)` — the
        //   C# unknown-listener form — and the per-peer service
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

    fn send_raw_transaction(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let request = RawTransactionRequest::parse(params)?;
        let relay_result = rpc_relay::relay_transaction(server, request.transaction)?;
        rpc_relay::map_relay_result(relay_result)
    }

    fn submit_block(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let request = SubmitBlockRequest::parse(params)?;
        let relay_result = rpc_relay::relay_block(server, request.block)?;
        rpc_relay::map_relay_result(relay_result)
    }

    fn with_local_node<F>(server: &RpcServer, func: F) -> Result<Value, RpcException>
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
