//! Typed request parsing for utility RPC handlers.
//!
//! Utility endpoints are simple, but keeping parameter validation here keeps
//! handler bodies focused on inventory lookup and address validation.

use serde_json::Value;

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;

pub(super) struct NoParamsRequest;

impl NoParamsRequest {
    pub(super) fn parse(params: &[Value], method: &str) -> Result<Self, RpcException> {
        if params.is_empty() {
            Ok(Self)
        } else {
            Err(RpcException::from(
                RpcError::invalid_params().with_data(format!("{method} expects no parameters")),
            ))
        }
    }
}

pub(super) struct ValidateAddressRequest {
    pub(super) address: String,
}

impl ValidateAddressRequest {
    pub(super) fn parse(params: &[Value]) -> Result<Self, RpcException> {
        let address = params.first().and_then(Value::as_str).ok_or_else(|| {
            RpcException::from(RpcError::invalid_params().with_data("address parameter required"))
        })?;
        Ok(Self {
            address: address.to_string(),
        })
    }
}
