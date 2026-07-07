//! Generic positional parameter parsing helpers.

use std::str::FromStr;

use crate::server::rpc_exception::RpcException;
use serde_json::Value;

use super::errors::invalid_params;

/// Typed request for JSON-RPC methods that accept no positional parameters.
#[derive(Debug)]
pub struct NoParamsRequest;

impl NoParamsRequest {
    /// Rejects any supplied positional parameter and names the owning method in
    /// the C#-compatible invalid-params message.
    #[inline]
    pub fn parse(params: &[Value], method: &str) -> Result<Self, RpcException> {
        if params.is_empty() {
            Ok(Self)
        } else {
            Err(invalid_params(format!("{method} expects no parameters")))
        }
    }
}

fn optional_unsigned_param<T>(
    value: Option<&Value>,
    default: T,
    message: impl Into<String>,
) -> Result<T, RpcException>
where
    T: Copy + TryFrom<u64> + FromStr,
{
    let message = message.into();
    let Some(value) = value else {
        return Ok(default);
    };
    match value {
        Value::Null => Ok(default),
        Value::Number(number) => number
            .as_u64()
            .and_then(|value| T::try_from(value).ok())
            .ok_or_else(|| invalid_params(message)),
        Value::String(text) => text
            .trim()
            .parse::<T>()
            .map_err(|_| invalid_params(message)),
        _ => Err(invalid_params(message)),
    }
}

/// Extracts a string parameter from JSON-RPC params.
#[inline]
pub fn expect_string_param(
    params: &[Value],
    index: usize,
    method: &str,
) -> Result<String, RpcException> {
    expect_string_param_with_message(
        params,
        index,
        format!("{} expects string parameter {}", method, index + 1),
    )
}

/// Extracts a string parameter from JSON-RPC params with a custom error message.
#[inline]
pub fn expect_string_param_with_message(
    params: &[Value],
    index: usize,
    message: impl Into<String>,
) -> Result<String, RpcException> {
    let message = message.into();
    params
        .get(index)
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .ok_or_else(|| invalid_params(message))
}

/// Extracts a u32 parameter from JSON-RPC params.
#[inline]
pub fn expect_u32_param(params: &[Value], index: usize, method: &str) -> Result<u32, RpcException> {
    expect_u32_param_with_message(
        params,
        index,
        format!("{} expects integer parameter {}", method, index + 1),
    )
}

/// Extracts a u32 parameter from JSON-RPC params with a custom error message.
#[inline]
pub fn expect_u32_param_with_message(
    params: &[Value],
    index: usize,
    message: impl Into<String>,
) -> Result<u32, RpcException> {
    let message = message.into();
    let value = params
        .get(index)
        .ok_or_else(|| invalid_params(message.clone()))?;
    value
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| invalid_params(message))
}

/// Extracts a u64 parameter from JSON-RPC params.
#[inline]
pub fn expect_u64_param(params: &[Value], index: usize, method: &str) -> Result<u64, RpcException> {
    let value = params.get(index).ok_or_else(|| {
        invalid_params(format!(
            "{} expects integer parameter {}",
            method,
            index + 1
        ))
    })?;
    value.as_u64().ok_or_else(|| {
        invalid_params(format!(
            "{} expects integer parameter {}",
            method,
            index + 1
        ))
    })
}

/// Extracts an optional u64 parameter, accepting JSON numbers, numeric strings,
/// or null/missing for the supplied default.
#[inline]
pub fn optional_u64_param(
    value: Option<&Value>,
    default: u64,
    message: impl Into<String>,
) -> Result<u64, RpcException> {
    optional_unsigned_param(value, default, message)
}

/// Extracts an optional usize parameter, accepting JSON numbers, numeric
/// strings, or null/missing for the supplied default.
#[inline]
pub fn optional_usize_param(
    value: Option<&Value>,
    default: usize,
    message: impl Into<String>,
) -> Result<usize, RpcException> {
    optional_unsigned_param(value, default, message)
}

/// Parses an optional boolean parameter (defaults to false).
#[inline]
pub fn parse_verbose(param: Option<&Value>) -> Result<bool, RpcException> {
    match param {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(Value::Number(n)) => Ok(n.as_i64().map(|v| v != 0).unwrap_or(false)),
        Some(_) => Err(invalid_params("verbose must be a boolean")),
    }
}
