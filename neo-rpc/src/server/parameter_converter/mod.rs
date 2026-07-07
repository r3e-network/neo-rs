//! # neo-rpc::server::parameter_converter
//!
//! RPC parameter parsing and type conversion helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `scalar`: Scalar and byte-like RPC conversion implementations.
//! - `signers`: RPC signer parameter parsing helpers.
//! - `tests`: Module-local tests and regression coverage.

use neo_execution::contract_parameter::ContractParameter;
use neo_primitives::UInt160;
use neo_serialization::json::{JArray, JObject, JToken};

use super::model::SignersAndWitnesses;
use super::model::{Address, BlockHashOrIndex, ContractNameOrHashOrId};
use super::rpc_error::RpcError;
use super::rpc_exception::RpcException;

mod scalar;
mod signers;
#[cfg(test)]
#[path = "../../tests/server/core/parameter_converter.rs"]
mod tests;

/// Context supplied when converting RPC parameters.
#[derive(Debug, Clone, Copy)]
pub struct ConversionContext {
    pub address_version: u8,
}

impl ConversionContext {
    pub const fn new(address_version: u8) -> Self {
        Self { address_version }
    }
}

/// Trait implemented by types that can be constructed from a JSON-RPC token.
pub trait RpcConvertible: Sized {
    fn from_token(token: &JToken, ctx: &ConversionContext) -> Result<Self, RpcException>;
}

pub struct ParameterConverter;

impl ParameterConverter {
    pub fn convert<T: RpcConvertible>(
        token: &JToken,
        ctx: &ConversionContext,
    ) -> Result<T, RpcException> {
        T::from_token(token, ctx)
    }
}

impl RpcConvertible for Address {
    fn from_token(token: &JToken, ctx: &ConversionContext) -> Result<Self, RpcException> {
        let text = expect_string(token, "Expected address string")?;
        parse_address(&text, ctx.address_version)
    }
}

impl RpcConvertible for Vec<Address> {
    fn from_token(token: &JToken, ctx: &ConversionContext) -> Result<Self, RpcException> {
        let array = expect_array(token)?;
        let mut result = Self::with_capacity(array.count());
        for (index, item) in array.children().iter().enumerate() {
            let token = item
                .as_ref()
                .ok_or_else(|| invalid_params(format!("Null address entry at index {index}")))?;
            result.push(<Address as RpcConvertible>::from_token(token, ctx)?);
        }
        Ok(result)
    }
}

impl RpcConvertible for BlockHashOrIndex {
    fn from_token(token: &JToken, _ctx: &ConversionContext) -> Result<Self, RpcException> {
        match token {
            JToken::Number(value) => {
                if value.is_nan() || value.is_infinite() {
                    return Err(invalid_params(format!(
                        "Invalid block index value: {}",
                        token.to_string_value()
                    )));
                }
                let rounded = value.round();
                if (value - rounded).abs() > f64::EPSILON {
                    return Err(invalid_params(format!(
                        "Invalid block index value: {}",
                        token.to_string_value()
                    )));
                }
                if rounded < 0.0 || rounded > f64::from(u32::MAX) {
                    return Err(invalid_params(format!(
                        "Invalid block index value: {}",
                        token.to_string_value()
                    )));
                }
                Ok(Self::from_index(rounded as u32))
            }
            JToken::String(text) => Self::try_parse(text)
                .ok_or_else(|| invalid_params(format!("Invalid block hash or index: {text}"))),
            _ => Err(invalid_params("Expected block hash or index string")),
        }
    }
}

impl RpcConvertible for ContractNameOrHashOrId {
    fn from_token(token: &JToken, _ctx: &ConversionContext) -> Result<Self, RpcException> {
        match token {
            JToken::Number(value) => {
                if value.is_nan() || value.is_infinite() {
                    return Err(invalid_params(format!(
                        "Invalid contract identifier: {}",
                        token.to_string_value()
                    )));
                }
                let rounded = value.round();
                if (value - rounded).abs() > f64::EPSILON {
                    return Err(invalid_params(format!(
                        "Invalid contract identifier: {}",
                        token.to_string_value()
                    )));
                }
                if rounded < f64::from(i32::MIN) || rounded > f64::from(i32::MAX) {
                    return Err(invalid_params(format!(
                        "Invalid contract identifier: {}",
                        token.to_string_value()
                    )));
                }
                Ok(Self::from_id(rounded as i32))
            }
            JToken::String(text) => Self::try_parse(text)
                .ok_or_else(|| invalid_params(format!("Invalid contract identifier: {text}"))),
            _ => Err(invalid_params("Expected contract identifier string")),
        }
    }
}

impl RpcConvertible for SignersAndWitnesses {
    fn from_token(token: &JToken, ctx: &ConversionContext) -> Result<Self, RpcException> {
        signers::parse_signers_and_witnesses(token, ctx)
    }
}

impl RpcConvertible for Vec<ContractParameter> {
    fn from_token(token: &JToken, _ctx: &ConversionContext) -> Result<Self, RpcException> {
        let array = expect_array(token)?;
        let mut parameters = Self::with_capacity(array.count());
        for (index, item) in array.children().iter().enumerate() {
            let token = item.as_ref().ok_or_else(|| {
                invalid_params(format!("Invalid contract parameter at index {index}"))
            })?;
            let value = jtoken_to_serde(token);
            let parameter = ContractParameter::from_json(&value).map_err(|e| {
                invalid_params(format!("Invalid contract parameter at index {index}: {e}"))
            })?;
            parameters.push(parameter);
        }
        Ok(parameters)
    }
}

pub(super) fn parse_address(text: &str, address_version: u8) -> Result<Address, RpcException> {
    let mut result = None;
    if UInt160::try_parse(text, &mut result) {
        if let Some(hash) = result {
            return Ok(Address::new(hash, address_version));
        }
    }

    neo_wallets::wallet_helper::WalletAddress::to_script_hash(text, address_version)
        .map(|hash| Address::new(hash, address_version))
        .map_err(|_| invalid_params(format!("Invalid address: {text}")))
}

pub(super) fn parse_uint160(text: &str) -> Result<UInt160, RpcException> {
    let mut result = None;
    if UInt160::try_parse(text, &mut result) {
        if let Some(value) = result {
            return Ok(value);
        }
    }
    Err(invalid_params(format!("Invalid UInt160 value: {text}")))
}

/// Converts a `JToken` into a `serde_json::Value` for downstream APIs that expect serde JSON.
pub(super) fn jtoken_to_serde(token: &JToken) -> serde_json::Value {
    match token {
        JToken::Null => serde_json::Value::Null,
        JToken::Boolean(b) => serde_json::Value::Bool(*b),
        JToken::Number(n) => serde_json::Number::from_f64(*n)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        JToken::String(s) => serde_json::Value::String(s.clone()),
        JToken::Array(arr) => serde_json::Value::Array(
            arr.children()
                .iter()
                .map(|item| {
                    item.as_ref()
                        .map_or(serde_json::Value::Null, jtoken_to_serde)
                })
                .collect(),
        ),
        JToken::Object(obj) => {
            let mut map = serde_json::Map::new();
            for (key, value) in obj.iter() {
                map.insert(
                    key.clone(),
                    value
                        .as_ref()
                        .map_or(serde_json::Value::Null, jtoken_to_serde),
                );
            }
            serde_json::Value::Object(map)
        }
    }
}

pub(super) fn expect_array(token: &JToken) -> Result<&JArray, RpcException> {
    match token {
        JToken::Array(array) => Ok(array),
        _ => Err(invalid_params("Expected JSON array")),
    }
}

pub(super) fn expect_object(token: &JToken) -> Result<&JObject, RpcException> {
    match token {
        JToken::Object(obj) => Ok(obj),
        _ => Err(invalid_params("Expected JSON object")),
    }
}

pub(super) fn expect_string(
    token: &JToken,
    context: impl Into<String>,
) -> Result<String, RpcException> {
    token
        .as_string()
        .ok_or_else(|| invalid_params(context.into()))
}

pub(super) fn numeric_from_token(token: &JToken) -> Result<f64, RpcException> {
    match token {
        JToken::String(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Ok(0.0);
            }
            trimmed
                .parse::<f64>()
                .map_err(|_| invalid_params("Expected numeric value"))
        }
        _ => token
            .as_number()
            .ok_or_else(|| invalid_params("Expected numeric value")),
    }
}

pub(super) fn invalid_params<T: Into<String>>(message: T) -> RpcException {
    RpcException::from(RpcError::invalid_params().with_data(message.into()))
}
