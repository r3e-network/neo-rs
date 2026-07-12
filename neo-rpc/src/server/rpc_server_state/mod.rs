//! # neo-rpc::server::rpc_server_state
//!
//! State-service RPC endpoint handlers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `proof`: State proof RPC handlers and proof payload codec.
//! - `request`: Typed JSON-RPC request parsing helpers.
//! - `response`: Typed JSON-RPC response construction helpers.
//! - `roots`: State-root and StateService height RPC handlers.
//! - `state_queries`: Historical state lookup through frozen provider views.
//! - `support`: State-provider factory lookup and RPC error projection.
//! - `tests`: Module-local tests and regression coverage.

use crate::server::rpc_server::RpcHandler;

mod proof;
mod request;
mod response;
mod roots;
mod state_queries;
mod support;

/// RPC handler group for StateService methods.
pub struct RpcServerState;

impl RpcServerState {
    /// Register StateService RPC handlers.
    pub fn register_handlers() -> Vec<RpcHandler> {
        super::rpc_handlers![
            "getstateheight" => Self::get_state_height,
            "getstateroot" => Self::get_state_root,
            "getproof" => Self::get_proof,
            "verifyproof" => Self::verify_proof,
            "getstate" => Self::get_state,
            "findstates" => Self::find_states,
        ]
    }
}

#[cfg(test)]
#[path = "../../tests/server/handlers/rpc_server_state.rs"]
mod tests;
