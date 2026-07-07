//! Typed request parsing for token-tracker RPC handlers.

use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::{
    expect_script_hash_or_address_param, invalid_params, optional_u64_param,
};
use neo_primitives::UInt160;
use neo_primitives::hex_util;
use serde_json::Value;

pub(super) struct AccountRequest {
    pub(super) script_hash: UInt160,
}

impl AccountRequest {
    pub(super) fn parse(
        params: &[Value],
        method: &str,
        address_version: u8,
    ) -> Result<Self, RpcException> {
        Ok(Self {
            script_hash: expect_script_hash_or_address_param(params, 0, method, address_version)?,
        })
    }
}

pub(super) struct TransferHistoryRequest {
    pub(super) script_hash: UInt160,
    pub(super) start: u64,
    pub(super) end: u64,
}

impl TransferHistoryRequest {
    pub(super) fn parse(
        params: &[Value],
        method: &str,
        address_version: u8,
    ) -> Result<Self, RpcException> {
        let script_hash = expect_script_hash_or_address_param(params, 0, method, address_version)?;
        let now_ms = current_time_millis();
        let start_time = parse_optional_u64(params.get(1))?;
        let end_time = parse_optional_u64(params.get(2))?;
        let start = if start_time == 0 {
            now_ms.saturating_sub(7 * 24 * 60 * 60 * 1000)
        } else {
            start_time
        };
        let end = if end_time == 0 { now_ms } else { end_time };
        if end < start {
            return Err(invalid_params("endTime must be >= startTime"));
        }
        Ok(Self {
            script_hash,
            start,
            end,
        })
    }
}

pub(super) struct Nep11PropertiesRequest {
    pub(super) script_hash: UInt160,
    pub(super) token_id: Vec<u8>,
}

impl Nep11PropertiesRequest {
    pub(super) fn parse(params: &[Value], address_version: u8) -> Result<Self, RpcException> {
        Ok(Self {
            script_hash: expect_script_hash_or_address_param(
                params,
                0,
                "getnep11properties",
                address_version,
            )?,
            token_id: parse_token_id_param(params, 1, "getnep11properties")?,
        })
    }
}

fn parse_optional_u64(value: Option<&Value>) -> Result<u64, RpcException> {
    optional_u64_param(value, 0, "Expected unsigned integer")
}

fn parse_token_id_param(
    params: &[Value],
    index: usize,
    method: &str,
) -> Result<Vec<u8>, RpcException> {
    let text = params
        .get(index)
        .and_then(|value| value.as_str())
        .ok_or_else(|| invalid_params(format!("{method} requires tokenId parameter")))?;
    hex_util::decode_hex(text).map_err(|_| invalid_params("Invalid tokenId"))
}

fn current_time_millis() -> u64 {
    neo_primitives::time::now_millis()
}
