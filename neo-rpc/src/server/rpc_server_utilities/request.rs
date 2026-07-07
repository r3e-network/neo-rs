//! Typed request parsing for utility RPC handlers.
//!
//! Utility endpoints are simple, but keeping parameter validation here keeps
//! handler bodies focused on inventory lookup and address validation.

use serde_json::Value;

use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::invalid_params;

pub(super) use crate::server::rpc_helpers::NoParamsRequest;

pub(super) struct ValidateAddressRequest {
    pub(super) address: String,
}

impl ValidateAddressRequest {
    pub(super) fn parse(params: &[Value]) -> Result<Self, RpcException> {
        let address = params
            .first()
            .and_then(Value::as_str)
            .ok_or_else(|| invalid_params("address parameter required"))?;
        Ok(Self {
            address: address.to_string(),
        })
    }
}
