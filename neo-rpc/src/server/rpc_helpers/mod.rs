//! # neo-rpc::server::rpc_helpers
//!
//! Shared helper functions for RPC handler implementations.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `tests`: Module-local tests and regression coverage.

use neo_io::Serializable;
use neo_primitives::{UInt160, UInt256};
use serde_json::Value;
use std::str::FromStr;

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};

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

/// Creates an RpcException for invalid parameters.
#[inline]
pub fn invalid_params(message: impl Into<String>) -> RpcException {
    RpcException::from(RpcError::invalid_params().with_data(message.into()))
}

/// Creates an RpcException for internal server errors.
#[inline]
pub fn internal_error(message: impl ToString) -> RpcException {
    RpcException::from(RpcError::internal_server_error().with_data(message.to_string()))
}

/// Serializes a Neo wire-compatible payload and encodes it with standard Base64.
#[inline]
pub fn serialize_to_base64<T>(value: &T) -> Result<String, RpcException>
where
    T: Serializable,
{
    crate::serialization::serializable_to_base64(value).map_err(internal_error)
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

/// Parses a UInt160 script hash or wallet address using the configured address version.
#[inline]
pub fn parse_script_hash_or_address(
    text: &str,
    address_version: u8,
) -> Result<UInt160, RpcException> {
    parse_script_hash_or_address_with_error(text, address_version, |_| {
        invalid_params(format!("Invalid address: {text}"))
    })
}

/// Parses a UInt160 script hash or wallet address with caller-owned address errors.
#[inline]
pub fn parse_script_hash_or_address_with_error(
    text: &str,
    address_version: u8,
    map_address_error: impl FnOnce(neo_error::CoreError) -> RpcException,
) -> Result<UInt160, RpcException> {
    crate::client::parse_script_hash_or_address_inner(text, address_version)
        .map_err(map_address_error)
}

/// Extracts a UInt160 script hash or wallet address parameter.
#[inline]
pub fn expect_script_hash_or_address_param(
    params: &[Value],
    index: usize,
    method: &str,
    address_version: u8,
) -> Result<UInt160, RpcException> {
    let text = params
        .get(index)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_params(format!("{method} expects address parameter")))?;
    parse_script_hash_or_address(text, address_version)
}

/// Extracts and decodes a trimmed base64 parameter using the standard RPC byte error.
#[inline]
pub fn expect_base64_param(
    params: &[Value],
    index: usize,
    method: &str,
) -> Result<Vec<u8>, RpcException> {
    let text = params.get(index).and_then(Value::as_str).ok_or_else(|| {
        invalid_params(format!("{} expects base64 parameter {}", method, index + 1))
    })?;
    decode_trimmed_base64_text(text, "Invalid Base64-encoded bytes")
}

/// Extracts and decodes a trimmed base64 parameter with a custom decode error.
#[inline]
pub fn expect_base64_param_with_decode_message(
    params: &[Value],
    index: usize,
    method: &str,
    decode_error: impl Into<String>,
) -> Result<Vec<u8>, RpcException> {
    let text = expect_string_param(params, index, method)?;
    decode_trimmed_base64_text(&text, decode_error)
}

/// Extracts and decodes an exact base64 parameter with distinct missing
/// and invalid-value error messages.
#[inline]
pub fn expect_base64_param_with_messages(
    params: &[Value],
    index: usize,
    missing_message: impl Into<String>,
    decode_error: impl FnOnce(&str) -> String,
) -> Result<Vec<u8>, RpcException> {
    let text = expect_string_param_with_message(params, index, missing_message)?;
    decode_base64_text(&text, decode_error(&text))
}

/// Extracts and decodes an exact base64 parameter where missing and invalid
/// values share the same RPC error message.
#[inline]
pub fn expect_base64_param_with_message(
    params: &[Value],
    index: usize,
    message: impl Into<String>,
) -> Result<Vec<u8>, RpcException> {
    let message = message.into();
    let text = params
        .get(index)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_params(message.clone()))?;
    decode_base64_text(text, message)
}

/// Decodes an exact base64 string with a custom RPC error message.
#[inline]
pub fn decode_base64_text(
    text: &str,
    error_message: impl Into<String>,
) -> Result<Vec<u8>, RpcException> {
    BASE64_STANDARD
        .decode(text)
        .map_err(|_| invalid_params(error_message.into()))
}

/// Decodes a trimmed base64 string with a custom RPC error message.
#[inline]
pub fn decode_trimmed_base64_text(
    text: &str,
    error_message: impl Into<String>,
) -> Result<Vec<u8>, RpcException> {
    decode_base64_text(text.trim(), error_message)
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

/// Parses a UInt160 from JSON-RPC params.
#[inline]
pub fn parse_uint160(
    params: &[Value],
    index: usize,
    method: &str,
) -> Result<UInt160, RpcException> {
    let text = expect_string_param(params, index, method)?;
    UInt160::from_str(&text)
        .map_err(|err| invalid_params(format!("invalid UInt160 '{}': {}", text, err)))
}

/// Parses a UInt160 text value with an error label such as "script hash" or "UInt160".
#[inline]
pub fn parse_uint160_text_with_label(text: &str, label: &str) -> Result<UInt160, RpcException> {
    UInt160::from_str(text).map_err(|err| invalid_params(format!("invalid {label}: {err}")))
}

/// Extracts and parses a UInt160 parameter using caller-owned RPC error text.
#[inline]
pub fn expect_uint160_param_with_message(
    params: &[Value],
    index: usize,
    message: impl Into<String>,
    label: &str,
) -> Result<UInt160, RpcException> {
    let text = expect_string_param_with_message(params, index, message)?;
    parse_uint160_text_with_label(&text, label)
}

/// Parses a UInt256 from JSON-RPC params.
#[inline]
pub fn parse_uint256(
    params: &[Value],
    index: usize,
    method: &str,
) -> Result<UInt256, RpcException> {
    expect_uint256_param_with_message(
        params,
        index,
        format!("{} expects string parameter {}", method, index + 1),
        "UInt256",
    )
}

/// Parses a UInt256 text value with an error label such as "hash" or "UInt256".
#[inline]
pub fn parse_uint256_text_with_label(text: &str, label: &str) -> Result<UInt256, RpcException> {
    UInt256::from_str(text)
        .map_err(|err| invalid_params(format!("invalid {label} '{text}': {err}")))
}

/// Extracts and parses a UInt256 parameter using caller-owned RPC error text.
#[inline]
pub fn expect_uint256_param_with_message(
    params: &[Value],
    index: usize,
    message: impl Into<String>,
    label: &str,
) -> Result<UInt256, RpcException> {
    let text = expect_string_param_with_message(params, index, message)?;
    parse_uint256_text_with_label(&text, label)
}

/// Parses a hash (UInt256) parameter, accepting both hex and base64.
#[inline]
pub fn expect_hash_param(
    params: &[Value],
    index: usize,
    method: &str,
) -> Result<UInt256, RpcException> {
    let text = expect_string_param(params, index, method)?;
    // Try hex first, then base64
    UInt256::from_str(&text)
        .or_else(|_| {
            use base64::{Engine, engine::general_purpose::STANDARD};
            STANDARD
                .decode(&text)
                .ok()
                .and_then(|bytes| UInt256::from_bytes(&bytes).ok())
                .ok_or(())
        })
        .map_err(|_| {
            invalid_params(format!(
                "{} expects valid hash at parameter {}",
                method,
                index + 1
            ))
        })
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

#[cfg(test)]
#[path = "../../tests/server/core/rpc_helpers.rs"]
mod tests;
