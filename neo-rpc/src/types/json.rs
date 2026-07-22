//! Shared JSON collection and scalar codecs for RPC model records.

use std::str::FromStr;

use neo_error::CoreResult;
use neo_primitives::UInt256;
use neo_serialization::json::{JArray, JObject, JToken};
use thiserror::Error;

/// Error produced while decoding a typed field from an RPC JSON model.
#[derive(Debug, Error)]
pub(crate) enum JsonParseError {
    /// Required field is missing or has the wrong type.
    #[error("Missing or invalid '{field}' field")]
    MissingField { field: String },

    /// A numeric field cannot be represented by its target integer type.
    #[error("Field '{field}' is out of range for {ty}: {value}")]
    #[cfg(feature = "client")]
    OutOfRange {
        field: String,
        ty: String,
        value: String,
    },

    /// A scalar string has invalid contents.
    #[error("Invalid {name}: {value}")]
    InvalidValue { name: String, value: String },

    /// A token has the wrong JSON type.
    #[error("{0}")]
    InvalidType(String),

    /// A codec produced a domain-specific error message.
    #[error("{0}")]
    Other(String),
}

impl From<String> for JsonParseError {
    fn from(message: String) -> Self {
        if let Some(field) = message.strip_prefix("Missing or invalid '") {
            if let Some(field) = field.strip_suffix("' field") {
                return Self::MissingField {
                    field: field.to_string(),
                };
            }
        }
        if message.starts_with("Field '") && message.contains(" is out of range for ") {
            return Self::Other(message);
        }
        if let Some(rest) = message.strip_prefix("Invalid ") {
            if let Some((name, value)) = rest.split_once(": ") {
                return Self::InvalidValue {
                    name: name.to_string(),
                    value: value.to_string(),
                };
            }
        }
        Self::Other(message)
    }
}

impl From<&str> for JsonParseError {
    fn from(message: &str) -> Self {
        Self::from(message.to_string())
    }
}

impl From<JsonParseError> for String {
    fn from(error: JsonParseError) -> Self {
        error.to_string()
    }
}

/// Parses a token encoded as either a JSON number or a decimal string.
pub(crate) fn parse_number_or_string_token<T>(
    token: &JToken,
    value_name: &str,
    invalid_type_error: &str,
    from_number: impl FnOnce(f64) -> T,
) -> Result<T, JsonParseError>
where
    T: FromStr,
{
    if let Some(number) = token.as_number() {
        Ok(from_number(number))
    } else if let Some(text) = token.as_string() {
        text.parse::<T>().map_err(|_| JsonParseError::InvalidValue {
            name: format!("{value_name} value"),
            value: text.to_string(),
        })
    } else {
        Err(JsonParseError::InvalidType(invalid_type_error.to_string()))
    }
}

/// Builds an ordered JSON object array token.
pub(crate) fn object_array<T>(items: &[T], to_object: impl FnMut(&T) -> JObject) -> JToken {
    object_array_from_iter(items.iter().map(to_object))
}

/// Builds an ordered JSON object array token from an object iterator.
pub(crate) fn object_array_from_iter(objects: impl IntoIterator<Item = JObject>) -> JToken {
    JToken::Array(JArray::from(
        objects.into_iter().map(JToken::Object).collect::<Vec<_>>(),
    ))
}

/// Builds an ordered JSON array token.
pub(crate) fn token_array<T>(items: &[T], to_token: impl FnMut(&T) -> JToken) -> JToken {
    JToken::Array(JArray::from(items.iter().map(to_token).collect::<Vec<_>>()))
}

/// Parses a string array while preserving the established lossy model behavior.
pub(crate) fn parse_string_array_lossy(json: &JObject, field: &str) -> Vec<String> {
    json.get(field)
        .and_then(JToken::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.as_ref())
                .filter_map(JToken::as_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Parses a `UInt256` string array while preserving lossy model behavior.
pub(crate) fn parse_uint256_array_lossy(json: &JObject, field: &str) -> Vec<UInt256> {
    parse_string_array_lossy(json, field)
        .into_iter()
        .filter_map(|value| UInt256::parse(&value).ok())
        .collect()
}

/// Parses present entries from an optional JSON array.
pub(crate) fn parse_optional_present_token_array_strict<T>(
    json: &JObject,
    field: &str,
    parse: impl FnMut(&JToken) -> CoreResult<T>,
) -> CoreResult<Vec<T>> {
    json.get(field)
        .and_then(JToken::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.as_ref())
                .map(parse)
                .collect()
        })
        .unwrap_or_else(|| Ok(Vec::new()))
}

/// Parses a JSON object array while preserving the established lossy model behavior.
pub(crate) fn parse_object_array_lossy<T, E>(
    json: &JObject,
    field: &str,
    mut parse: impl FnMut(&JObject) -> Result<T, E>,
) -> Vec<T> {
    json.get(field)
        .and_then(JToken::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.as_ref())
                .filter_map(JToken::as_object)
                .filter_map(|object| parse(object).ok())
                .collect()
        })
        .unwrap_or_default()
}
