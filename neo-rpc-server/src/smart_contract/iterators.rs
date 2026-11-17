use serde_json::Value;

use crate::rpc_error::RpcError;
use crate::rpc_exception::RpcException;
use crate::rpc_server::RpcServer;

use super::helpers::{
    DEFAULT_RESPONSE_BUDGET_BYTES, expect_u32_param, expect_uuid_param, internal_error,
    invalid_params, stack_item_to_json,
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
    // R-7(a): a zero count used to be accepted as a pure TTL refresh, which let
    // a client hold an iterator session alive indefinitely without consuming it.
    if count == 0 {
        return Err(invalid_params("Invalid iterator items count 0"));
    }
    if (count as usize) > server.settings().max_iterator_result_items {
        return Err(invalid_params(format!(
            "Invalid iterator items count {count}"
        )));
    }
    server.purge_expired_sessions();
    let result = server
        .with_session_mut(&session_id, |session| {
            match session.traverse_iterator(&iterator_id, count as usize) {
                Ok(items) => {
                    // R-7(c): only a traversal that actually yields items counts
                    // as activity; empty scans must not extend the idle TTL.
                    if !items.is_empty() {
                        session.touch_idle();
                    }
                    let mut budget = DEFAULT_RESPONSE_BUDGET_BYTES;
                    let mut session_ref = Some(session);
                    let mut values = Vec::new();
                    for item in items {
                        values.push(stack_item_to_json(
                            &item,
                            session_ref.as_deref_mut(),
                            &mut budget,
                        )?);
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
    if server.terminate_session(&session_id) {
        Ok(Value::Bool(true))
    } else {
        Err(RpcException::from(RpcError::unknown_session()))
    }
}
