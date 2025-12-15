use serde_json::Value;

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_server::RpcServer;

use super::helpers::{
    expect_u32_param, expect_uuid_param, internal_error, invalid_params, stack_item_to_json,
};

pub(super) fn traverse_iterator(
    server: &RpcServer,
    params: &[Value],
) -> Result<Value, RpcException> {
    if !server.session_enabled() {
        return Err(RpcException::from(RpcError::sessions_disabled()));
    }
    let session_id = expect_uuid_param(params, 0, "traverseiterator")?;
    let iterator_id = expect_uuid_param(params, 1, "traverseiterator")?;
    let count = expect_u32_param(params, 2, "traverseiterator")?;
    if (count as usize) > server.settings().max_iterator_result_items {
        return Err(invalid_params(format!(
            "Invalid iterator items count {}",
            count
        )));
    }
    server.purge_expired_sessions();
    let result = server
        .with_session_mut(&session_id, |session| {
            session.reset_expiration();
            match session.traverse_iterator(&iterator_id, count as usize) {
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
    let session_id = expect_uuid_param(params, 0, "terminatesession")?;
    server.purge_expired_sessions();
    if !server.terminate_session(&session_id) {
        return Err(RpcException::from(RpcError::unknown_session()));
    }
    Ok(Value::Bool(true))
}
