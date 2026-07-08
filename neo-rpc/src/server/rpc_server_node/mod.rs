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
//! - `native_provider`: Native Ledger/Policy read seam for node endpoints.
//! - `relay`: Transaction and block relay endpoint handlers.
//! - `request`: Typed JSON-RPC request parsing helpers.
//! - `response`: C#-compatible node status and version response construction.
//! - `status`: Peer status endpoint handlers.
//! - `tests`: Module-local tests and regression coverage.
//! - `version`: Dynamic `getversion` policy lookup.

use crate::server::rpc_server::RpcHandler;

mod native_provider;
mod relay;
mod request;
mod response;
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
