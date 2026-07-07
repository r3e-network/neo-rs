//! Generic `JToken` helpers shared by typed RPC parameter converters.
//!
//! Domain converters own Neo-specific parsing. This module only owns JSON token
//! shape checks, numeric coercion, and conversion to `serde_json::Value` for
//! downstream APIs that already expect serde JSON.

use neo_serialization::json::{JArray, JObject, JToken};

use super::invalid_params;
use crate::server::rpc_exception::RpcException;

/// Converts a `JToken` into a `serde_json::Value` for downstream APIs.
pub(in crate::server::parameter_converter) fn jtoken_to_serde(token: &JToken) -> serde_json::Value {
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

pub(in crate::server::parameter_converter) fn expect_array(
    token: &JToken,
) -> Result<&JArray, RpcException> {
    match token {
        JToken::Array(array) => Ok(array),
        _ => Err(invalid_params("Expected JSON array")),
    }
}

pub(in crate::server::parameter_converter) fn expect_object(
    token: &JToken,
) -> Result<&JObject, RpcException> {
    match token {
        JToken::Object(obj) => Ok(obj),
        _ => Err(invalid_params("Expected JSON object")),
    }
}

pub(in crate::server::parameter_converter) fn expect_string(
    token: &JToken,
    context: impl Into<String>,
) -> Result<String, RpcException> {
    token
        .as_string()
        .ok_or_else(|| invalid_params(context.into()))
}

pub(in crate::server::parameter_converter) fn numeric_from_token(
    token: &JToken,
) -> Result<f64, RpcException> {
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
