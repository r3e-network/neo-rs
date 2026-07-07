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
//! - `inventory`: inventory payload traits and records.
//! - `request`: Typed JSON-RPC request parsing helpers.
//! - `response`: Utility RPC response construction helpers.
//! - `tests`: Module-local tests and regression coverage.

use serde_json::Value;

use super::rpc_exception::RpcException;
use super::rpc_server::{RpcHandler, RpcServer};

mod inventory;
mod request;
mod response;

use self::request::{NoParamsRequest, ValidateAddressRequest};
use self::response::validate_address_to_json;

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

    fn list_plugins_handler(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        NoParamsRequest::parse(params, "listplugins")?;
        Ok(server.list_plugins())
    }

    fn list_services_handler(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        NoParamsRequest::parse(params, "listservices")?;
        Ok(server.list_services())
    }

    fn validate_address_handler(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let request = ValidateAddressRequest::parse(params)?;
        Ok(server.validate_address(&request.address))
    }
}

impl RpcServer {
    /// Validate a Neo address against the node's configured address version.
    #[must_use]
    pub fn validate_address(&self, address: &str) -> Value {
        let address_version = self.system().settings().address_version;
        let is_valid =
            neo_wallets::wallet_helper::WalletAddress::to_script_hash(address, address_version)
                .is_ok();

        validate_address_to_json(address, is_valid)
    }
}

#[cfg(test)]
#[path = "../../tests/server/handlers/rpc_server_utilities.rs"]
mod tests;
