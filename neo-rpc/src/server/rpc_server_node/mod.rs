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
//! - `relay`: Transaction and block relay endpoint handlers.
//! - `request`: Typed JSON-RPC request parsing helpers.
//! - `status`: Peer status endpoint handlers.
//! - `tests`: Module-local tests and regression coverage.
//! - `version`: C#-compatible `getversion` response construction.

use crate::server::rpc_server::RpcHandler;

mod relay;
mod request;
mod status;
mod version;

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
}
