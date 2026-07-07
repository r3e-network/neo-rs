//! # neo-rpc::server::rpc_server_utilities
//!
//! Utility RPC endpoint handlers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `address`: address validation against the node address version.
//! - `inventory`: runtime service and plugin inventory lookup.
//! - `request`: Typed JSON-RPC request parsing helpers.
//! - `response`: Utility RPC response construction helpers.
//! - `tests`: Module-local tests and regression coverage.

use super::rpc_server::RpcHandler;

mod address;
mod inventory;
mod request;
mod response;

/// RPC handler group for utility methods.
pub struct RpcServerUtilities;

impl RpcServerUtilities {
    /// Register utility RPC handlers.
    pub fn register_handlers() -> Vec<RpcHandler> {
        super::rpc_handlers![
            "listplugins" => Self::list_plugins_handler,
            "listservices" => Self::list_services_handler,
            "validateaddress" => Self::validate_address_handler,
        ]
    }
}

#[cfg(test)]
#[path = "../../tests/server/handlers/rpc_server_utilities.rs"]
mod tests;
