//! jsonrpsee transport parameter and error codecs.
//!
//! The adapter root owns method registration. This module owns the wire-shape
//! conversion between jsonrpsee's raw parameter/error types and Neo's RPC error
//! catalog.

use jsonrpsee::types::{ErrorObjectOwned, Params};
use serde_json::Value;

use crate::server::rpc_error::RpcError;

/// Decode jsonrpsee raw params as the positional array expected by Neo RPC
/// handlers.
pub(super) fn parse_array_params(params: Params<'_>) -> Result<Vec<Value>, ErrorObjectOwned> {
    let Some(raw) = params.as_str() else {
        return Ok(Vec::new());
    };

    if raw.is_empty() {
        return Ok(Vec::new());
    }

    match serde_json::from_str::<Value>(raw) {
        Ok(Value::Array(values)) => Ok(values),
        Ok(_) => Err(error_object(RpcError::invalid_request())),
        Err(err) => Err(error_object(
            RpcError::invalid_params().with_data(err.to_string()),
        )),
    }
}

/// Project a Neo RPC error into jsonrpsee's owned JSON-RPC error object.
pub(super) fn error_object(error: RpcError) -> ErrorObjectOwned {
    ErrorObjectOwned::owned(
        error.code(),
        error.error_message(),
        error.data().map(std::string::ToString::to_string),
    )
}
