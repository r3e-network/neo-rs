//! Typed request parsing for StateService RPC handlers.
//!
//! Each struct represents the JSON-RPC parameters accepted by one handler or
//! handler family. Keeping C# binder quirks here keeps the handler body focused
//! on state-service reads and proof construction.

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::{expect_base64_param_with_message, expect_u32_param_with_message};
use neo_primitives::{UInt160, UInt256};
use serde_json::Value;

use super::support::MAX_FIND_RESULT_ITEMS;

pub(super) use crate::server::rpc_helpers::NoParamsRequest;

pub(super) struct StateRootRequest {
    pub(super) index: u32,
}

impl StateRootRequest {
    pub(super) fn parse(params: &[Value]) -> Result<Self, RpcException> {
        Ok(Self {
            index: expect_u32(params, 0, "getstateroot")?,
        })
    }
}

pub(super) struct StateKeyRequest {
    pub(super) root_hash: UInt256,
    pub(super) script_hash: UInt160,
    pub(super) key: Vec<u8>,
}

impl StateKeyRequest {
    pub(super) fn parse_get_proof(params: &[Value]) -> Result<Self, RpcException> {
        Self::parse(params, "getproof", "Base64 storage key")
    }

    pub(super) fn parse_get_state(params: &[Value]) -> Result<Self, RpcException> {
        Self::parse(params, "getstate", "Base64 storage key")
    }

    fn parse(params: &[Value], method: &str, key_descriptor: &str) -> Result<Self, RpcException> {
        Ok(Self {
            root_hash: parse_uint256(params, 0, method)?,
            script_hash: parse_uint160(params, 1, method)?,
            key: parse_base64(params, 2, method, key_descriptor)?,
        })
    }
}

pub(super) struct VerifyProofRequest {
    pub(super) root_hash: UInt256,
    pub(super) proof_bytes: Vec<u8>,
}

impl VerifyProofRequest {
    pub(super) fn parse(params: &[Value]) -> Result<Self, RpcException> {
        Ok(Self {
            root_hash: parse_uint256(params, 0, "verifyproof")?,
            proof_bytes: parse_base64(params, 1, "verifyproof", "Base64 proof payload")?,
        })
    }
}

pub(super) struct FindStatesRequest {
    pub(super) root_hash: UInt256,
    pub(super) script_hash: UInt160,
    pub(super) prefix: Vec<u8>,
    pub(super) from_key: Option<Vec<u8>>,
    pub(super) count: usize,
}

impl FindStatesRequest {
    pub(super) fn parse(params: &[Value]) -> Result<Self, RpcException> {
        Ok(Self {
            root_hash: parse_uint256(params, 0, "findstates")?,
            script_hash: parse_uint160(params, 1, "findstates")?,
            prefix: parse_base64(params, 2, "findstates", "Base64 key prefix")?,
            from_key: parse_optional_base64(params, 3, "findstates", "Base64 from-key")?,
            count: parse_find_count(params, 4)?,
        })
    }
}

pub(super) fn parse_uint256(
    params: &[Value],
    idx: usize,
    method: &str,
) -> Result<UInt256, RpcException> {
    parse_uint_parameter(params, idx, method, "UInt256", UInt256::parse)
}

pub(super) fn parse_uint160(
    params: &[Value],
    idx: usize,
    method: &str,
) -> Result<UInt160, RpcException> {
    parse_uint_parameter(params, idx, method, "UInt160", UInt160::parse)
}

fn parse_uint_parameter<T, E>(
    params: &[Value],
    idx: usize,
    method: &str,
    type_name: &str,
    parse: impl FnOnce(&str) -> Result<T, E>,
) -> Result<T, RpcException> {
    let value = params.get(idx).and_then(Value::as_str).ok_or_else(|| {
        RpcException::from(RpcError::invalid_params().with_data(format!(
            "{method} expects {type_name} parameter at index {idx}"
        )))
    })?;
    parse(value).map_err(|_| {
        RpcException::from(
            RpcError::invalid_params().with_data(format!("failed to parse {type_name} parameter")),
        )
    })
}

fn parse_base64(
    params: &[Value],
    idx: usize,
    method: &str,
    descriptor: &str,
) -> Result<Vec<u8>, RpcException> {
    expect_base64_param_with_message(
        params,
        idx,
        format!("{method} expects {descriptor} at index {idx}"),
    )
}

/// Parses an optional Base64 parameter: absent or `null` maps to
/// `None` (the C# binder's `byte[] key = null` default).
fn parse_optional_base64(
    params: &[Value],
    idx: usize,
    method: &str,
    descriptor: &str,
) -> Result<Option<Vec<u8>>, RpcException> {
    match params.get(idx) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => parse_base64(params, idx, method, descriptor).map(Some),
    }
}

/// Parses the optional `findstates` count with the C# binder's accepting
/// behavior: absent or `null` falls back to the C# parameter default
/// (`int count = 0`, i.e. `MaxFindResultItems`); present tokens go through the
/// `ParameterConverter.ToNumeric<int>` conversion; non-positive results select
/// the default page size and explicit values are capped at
/// [`MAX_FIND_RESULT_ITEMS`].
fn parse_find_count(params: &[Value], idx: usize) -> Result<usize, RpcException> {
    let requested = match params.get(idx) {
        None | Some(Value::Null) => 0i32,
        Some(value) => to_numeric_i32(value)?,
    };
    if requested <= 0 {
        return Ok(MAX_FIND_RESULT_ITEMS);
    }
    Ok((requested as usize).min(MAX_FIND_RESULT_ITEMS))
}

/// C# `ParameterConverter.ToNumeric<int>` (ParameterConverter.cs): funnels the
/// token through `JToken.AsNumber()` and requires the result to be an integral
/// value within `i32` range.
fn to_numeric_i32(value: &Value) -> Result<i32, RpcException> {
    let number = token_as_number(value);
    // C# checks the `int` range first and then `IsValidInteger` (an exact
    // integral remainder; NaN fails it). Infinity fails the range check; both
    // reject the same way here.
    if !(number >= f64::from(i32::MIN) && number <= f64::from(i32::MAX)) || number % 1.0 != 0.0 {
        return Err(RpcException::from(
            RpcError::invalid_params().with_data(format!("Invalid System.Int32 value: {value}")),
        ));
    }
    Ok(number as i32)
}

/// Neo.Json `JToken.AsNumber()`: numbers pass through, strings parse as
/// invariant floating-point text, booleans map to `1`/`0`, and every other token
/// is NaN.
fn token_as_number(value: &Value) -> f64 {
    match value {
        Value::Number(number) => number.as_f64().unwrap_or(f64::NAN),
        Value::String(text) => {
            if text.is_empty() {
                return 0.0;
            }
            let trimmed = text.trim();
            if trimmed.is_empty() {
                f64::NAN
            } else {
                trimmed.parse::<f64>().unwrap_or(f64::NAN)
            }
        }
        Value::Bool(flag) => {
            if *flag {
                1.0
            } else {
                0.0
            }
        }
        _ => f64::NAN,
    }
}

fn expect_u32(params: &[Value], idx: usize, method: &str) -> Result<u32, RpcException> {
    expect_u32_param_with_message(
        params,
        idx,
        format!("{method} expects unsigned integer parameter"),
    )
}
