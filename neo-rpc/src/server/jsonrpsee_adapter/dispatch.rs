//! Per-request jsonrpsee dispatch into the internal RPC registry.

use super::RpcAuthState;
use super::codec::error_object;
use super::context::JsonRpseeContext;
use crate::server::dispatch::Dispatch;
use crate::server::rpc_error::RpcError;
use jsonrpsee::server::Extensions;
use jsonrpsee::types::ErrorObjectOwned;
use serde_json::Value;

pub(super) fn dispatch_jsonrpsee_request(
    context: &JsonRpseeContext,
    method: &str,
    params: &[Value],
    extensions: &Extensions,
) -> Result<Value, ErrorObjectOwned> {
    let (server, handler) =
        Dispatch::resolve_rpc_handler(&context.server, context.disabled.as_ref(), method)
            .map_err(error_object)?;

    if handler.descriptor().requires_auth() {
        let authenticated = extensions
            .get::<RpcAuthState>()
            .is_some_and(|state| state.is_authenticated());
        if !server.read().rpc_auth_configured() || !authenticated {
            return Err(error_object(RpcError::access_denied()));
        }
    }

    Dispatch::invoke_rpc_handler(&server, handler, method, params).map_err(error_object)
}
