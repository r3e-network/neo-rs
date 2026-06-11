//! Shared RPC helper functions.
//!
//! This module consolidates common helper functions used across RPC server modules
//! to eliminate code duplication.

use neo_primitives::{UInt160, UInt256};
use neo_io::Serializable;
use serde_json::Value;
use std::str::FromStr;

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};

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
    T: Serializable + ?Sized,
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
    params
        .get(index)
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .ok_or_else(|| invalid_params(format!("{} expects string parameter {}", method, index + 1)))
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
    let value = params.get(index).ok_or_else(|| {
        invalid_params(format!(
            "{} expects integer parameter {}",
            method,
            index + 1
        ))
   })?;
    value
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| {
            invalid_params(format!(
                "{} expects integer parameter {}",
                method,
                index + 1
            ))
       })
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

/// Parses a UInt256 from JSON-RPC params.
#[inline]
pub fn parse_uint256(
    params: &[Value],
    index: usize,
    method: &str,
) -> Result<UInt256, RpcException> {
    let text = expect_string_param(params, index, method)?;
    UInt256::from_str(&text)
        .map_err(|err| invalid_params(format!("invalid UInt256 '{}': {}", text, err)))
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
            use base64::{engine::general_purpose::STANDARD, Engine};
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
        Some(_) => Err(invalid_params("verbose must be a boolean"))}
}
