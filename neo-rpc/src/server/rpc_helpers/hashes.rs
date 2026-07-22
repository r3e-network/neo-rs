//! UInt160, UInt256, and address parsing helpers.

use std::str::FromStr;

use base64::{Engine, engine::general_purpose::STANDARD};
use neo_primitives::{UInt160, UInt256};
use serde_json::Value;

use crate::server::rpc_exception::RpcException;

use super::errors::invalid_params;
use super::params::{expect_string_param, expect_string_param_with_message};

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
    crate::protocol::address::parse_script_hash_or_address(text, address_version)
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
    // Try hex first, then base64.
    UInt256::from_str(&text)
        .or_else(|_| {
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
