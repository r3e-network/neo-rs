use serde_json::Value;

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_server::RpcServer;

use super::helpers::{internal_error, invalid_params};
use super::request::{TerminateSessionRequest, TraverseIteratorRequest};
use super::response::stack_item_to_json;

pub(super) fn traverse_iterator(
    server: &RpcServer,
    params: &[Value],
) -> Result<Value, RpcException> {
    if !server.session_enabled() {
        return Err(RpcException::from(RpcError::sessions_disabled()));
    }
    let request = TraverseIteratorRequest::parse(params)?;
    if (request.count as usize) > server.settings().max_iterator_result_items {
        return Err(invalid_params(format!(
            "Invalid iterator items count {}",
            request.count
        )));
    }
    server.purge_expired_sessions();
    let result = server
        .with_session_mut(&request.session_id, |session| {
            session.reset_expiration();
            match session.traverse_iterator(&request.iterator_id, request.count as usize) {
                Ok(items) => {
                    let mut session_ref = Some(session);
                    let mut values = Vec::new();
                    for item in items {
                        values.push(stack_item_to_json(&item, session_ref.as_deref_mut())?);
                    }
                    Ok(Value::Array(values))
                }
                Err(message) if message == "Unknown iterator" => {
                    Err(RpcException::from(RpcError::unknown_iterator()))
                }
                Err(message) => Err(internal_error(message)),
            }
        })
        .ok_or_else(|| RpcException::from(RpcError::unknown_session()))??;

    Ok(result)
}

pub(super) fn terminate_session(
    server: &RpcServer,
    params: &[Value],
) -> Result<Value, RpcException> {
    if !server.session_enabled() {
        return Err(RpcException::from(RpcError::sessions_disabled()));
    }
    let request = TerminateSessionRequest::parse(params)?;
    server.purge_expired_sessions();
    Ok(Value::Bool(server.terminate_session(&request.session_id)))
}
