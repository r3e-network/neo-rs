//! Typed request parsing for ApplicationLogs RPC handlers.
//!
//! ApplicationLogs exposes a small JSON-RPC surface, but keeping hash and
//! trigger-filter parsing here keeps the handler focused on service lookup and
//! log retrieval.

use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::{expect_uint256_param_with_message, invalid_params};
use neo_primitives::UInt256;
use serde_json::Value;

pub(super) struct ApplicationLogRequest {
    pub(super) hash: UInt256,
    pub(super) trigger_filter: Option<String>,
}

impl ApplicationLogRequest {
    pub(super) fn parse(params: &[Value]) -> Result<Self, RpcException> {
        Ok(Self {
            hash: expect_hash_param(params, 0)?,
            trigger_filter: parse_trigger_filter(params.get(1))?,
        })
    }
}

fn expect_hash_param(params: &[Value], index: usize) -> Result<UInt256, RpcException> {
    expect_uint256_param_with_message(
        params,
        index,
        format!("getapplicationlog expects string parameter {}", index + 1),
        "hash",
    )
}

fn parse_trigger_filter(value: Option<&Value>) -> Result<Option<String>, RpcException> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) if text.trim().is_empty() => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.trim().to_string())),
        _ => Err(invalid_params(
            "getapplicationlog expects string parameter 2",
        )),
    }
}
