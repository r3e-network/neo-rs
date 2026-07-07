//! Base64 and Neo wire-payload encoding helpers.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_io::Serializable;
use serde_json::Value;

use crate::server::rpc_exception::RpcException;

use super::errors::{internal_error, invalid_params};
use super::params::{expect_string_param, expect_string_param_with_message};

/// Serializes a Neo wire-compatible payload and encodes it with standard Base64.
#[inline]
pub fn serialize_to_base64<T>(value: &T) -> Result<String, RpcException>
where
    T: Serializable,
{
    crate::serialization::serializable_to_base64(value).map_err(internal_error)
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
