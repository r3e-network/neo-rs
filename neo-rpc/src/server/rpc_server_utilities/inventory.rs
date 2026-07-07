//! Runtime inventory lookup for utility RPC methods.
//!
//! `listplugins` and `listservices` gather local service/plugin facts here and
//! delegate response-shape construction to the sibling response module.

use serde_json::Value;

use crate::server::rpc_exception::RpcException;
use crate::server::rpc_server::RpcServer;
use crate::server::rpc_server_utilities::RpcServerUtilities;
use crate::server::rpc_server_utilities::request::NoParamsRequest;

mod plugins;
mod services;

impl RpcServerUtilities {
    pub(super) fn list_plugins_handler(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        NoParamsRequest::parse(params, "listplugins")?;
        Ok(server.list_plugins())
    }

    pub(super) fn list_services_handler(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        NoParamsRequest::parse(params, "listservices")?;
        Ok(server.list_services())
    }
}
