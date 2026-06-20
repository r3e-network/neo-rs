use serde_json::Value;

use super::rpc_error::RpcError;
use super::rpc_exception::RpcException;
use super::rpc_server::{RpcHandler, RpcServer};

mod inventory;

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
        Self::expect_no_params(params, "listplugins")?;
        Ok(server.list_plugins())
    }

    fn list_services_handler(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        Self::expect_no_params(params, "listservices")?;
        Ok(server.list_services())
    }

    fn validate_address_handler(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let address = params.first().and_then(|v| v.as_str()).ok_or_else(|| {
            RpcException::from(RpcError::invalid_params().with_data("address parameter required"))
        })?;
        Ok(server.validate_address(address))
    }

    fn expect_no_params(params: &[Value], method: &str) -> Result<(), RpcException> {
        if params.is_empty() {
            Ok(())
        } else {
            Err(RpcException::from(
                RpcError::invalid_params().with_data(format!("{method} expects no parameters")),
            ))
        }
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

        serde_json::json!({
            "address": address,
            "isvalid": is_valid})
    }
}

#[cfg(test)]
mod tests;
