//! # neo-rpc::server::rpc_server_oracle
//!
//! Oracle RPC endpoint handlers.
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
//! - `response`: Oracle RPC response construction helpers.
//! - `submission`: Oracle response submission endpoint implementation.
//! - `tests`: Module-local tests and regression coverage.

use crate::server::rpc_server::RpcHandler;
use neo_oracle_service::OracleService;

type RpcOracleService = OracleService<crate::server::NodeContext>;

mod request;
mod response;
mod submission;

/// RPC handler group for Oracle service methods.
pub struct RpcServerOracle;

impl RpcServerOracle {
    /// Register Oracle RPC handlers.
    pub fn register_handlers() -> Vec<RpcHandler> {
        super::rpc_handlers![
            "submitoracleresponse" => Self::submit_oracle_response,
        ]
    }
}

#[cfg(test)]
#[path = "../../tests/server/handlers/rpc_server_oracle.rs"]
mod tests;
