use base64::{Engine as _, engine::general_purpose};
use neo_config::ProtocolSettings;
use neo_error::{CoreError, CoreResult};
use neo_payloads::OracleResponseCode;
use neo_primitives::{UInt160, UInt256, strip_hex_prefix};
use neo_serialization::json::{JArray, JObject, JToken};
use neo_wallets::wallet_helper::WalletAddress as WalletHelper;
use num_bigint::BigInt;
use serde_json::Value as JsonValue;
use std::{fmt::Display, str::FromStr};
use thiserror::Error;

/// Error type for JSON-RPC client utility parsing operations.
///
/// Replaces the previous `Result<_, String>` returns across the 24
/// `parsing.rs` call-sites. The `MissingField`/`OutOfRange`/`InvalidValue`
/// variants cover the most common failure modes; the `Other` variant is a
/// fallback for `From<String>` conversions of legacy `format!`-based errors.
#[derive(Debug, Error)]
pub enum JsonParseError {
    /// Required field is missing from the JSON object or has the wrong type.
    #[error("Missing or invalid '{field}' field")]
    MissingField {
        /// The field name that was missing or invalid.
        field: String,
    },

    /// A numeric field was out of range for the requested integer type.
    #[error("Field '{field}' is out of range for {ty}: {value}")]
    OutOfRange {
        /// The field name.
        field: String,
        /// Target type (`u64`, `u32`, `u16`, …).
        ty: String,
        /// The value that was out of range.
        value: String,
    },

    /// A string field could not be parsed as the requested scalar type
    /// (e.g. `BigInt`, `UInt256`, `u64`).
    #[error("Invalid {name}: {value}")]
    InvalidValue {
        /// The semantic name of the value (e.g. `"amount"`, `"block index"`).
        name: String,
        /// The string that failed to parse.
        value: String,
    },

    /// A token had the wrong JSON type (e.g. expected `String`, got `Number`).
    #[error("{0}")]
    InvalidType(String),

    /// Catch-all for legacy `format!()`-based error messages; new code
    /// should construct the right variant directly.
    #[error("{0}")]
    Other(String),
}

impl From<String> for JsonParseError {
    fn from(message: String) -> Self {
        // Try to bucket legacy strings into a structured variant based on
        // common prefixes. The `Other` variant is the fallback.
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
    fn from(err: JsonParseError) -> Self {
        err.to_string()
    }
}

/// Reads a required string field from a JSON object.
pub fn required_string(json: &JObject, field: &str) -> Result<String, JsonParseError> {
    json.get(field)
        .and_then(JToken::as_string)
        .ok_or_else(|| JsonParseError::MissingField {
            field: field.to_string(),
        })
}

/// Reads an optional string field from a JSON object.
pub fn optional_string(json: &JObject, field: &str) -> Option<String> {
    json.get(field).and_then(JToken::as_string)
}

/// Builds a string token for `Some` values and `Null` for missing values.
pub fn optional_string_or_null(value: Option<impl Into<String>>) -> JToken {
    value.map_or(JToken::Null, |value| JToken::String(value.into()))
}

/// Inserts a string token for `Some` values and `Null` for missing values.
pub fn insert_optional_string(json: &mut JObject, field: &str, value: Option<impl Into<String>>) {
    json.insert(field.to_string(), optional_string_or_null(value));
}

/// Reads a required numeric field as a `u64`.
pub fn required_u64_number(json: &JObject, field: &str) -> Result<u64, JsonParseError> {
    let value = json.get(field).and_then(JToken::as_number).ok_or_else(|| {
        JsonParseError::MissingField {
            field: field.to_string(),
        }
    })?;
    if value < 0.0 || value > u64::MAX as f64 || value.fract() != 0.0 {
        return Err(JsonParseError::OutOfRange {
            field: field.to_string(),
            ty: "u64".to_string(),
            value: value.to_string(),
        });
    }
    Ok(value as u64)
}

/// Reads a required numeric field as a `u32`.
pub fn required_u32_number(json: &JObject, field: &str) -> Result<u32, JsonParseError> {
    let value = json.get(field).and_then(JToken::as_number).ok_or_else(|| {
        JsonParseError::MissingField {
            field: field.to_string(),
        }
    })?;
    if value < 0.0 || value > f64::from(u32::MAX) || value.fract() != 0.0 {
        return Err(JsonParseError::OutOfRange {
            field: field.to_string(),
            ty: "u32".to_string(),
            value: value.to_string(),
        });
    }
    Ok(value as u32)
}

/// Reads a required numeric field as a `u16`.
pub fn required_u16_number(json: &JObject, field: &str) -> Result<u16, JsonParseError> {
    let value = json.get(field).and_then(JToken::as_number).ok_or_else(|| {
        JsonParseError::MissingField {
            field: field.to_string(),
        }
    })?;
    if value < 0.0 || value > f64::from(u16::MAX) || value.fract() != 0.0 {
        return Err(JsonParseError::OutOfRange {
            field: field.to_string(),
            ty: "u16".to_string(),
            value: value.to_string(),
        });
    }
    Ok(value as u16)
}

/// Parses a token that may be encoded as either a JSON number or decimal string.
pub fn parse_number_or_string_token<T>(
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

/// Reads a required decimal integer string field as a `BigInt`.
pub fn required_bigint_string(
    json: &JObject,
    field: &str,
    value_name: &str,
) -> Result<BigInt, JsonParseError> {
    let value = required_string(json, field)?;
    BigInt::from_str(&value).map_err(|_| JsonParseError::InvalidValue {
        name: value_name.to_string(),
        value,
    })
}

/// Reads a required `UInt256` string field.
pub fn required_uint256(json: &JObject, field: &str) -> CoreResult<UInt256> {
    let value = required_string(json, field).map_err(|e| CoreError::other(e.to_string()))?;
    UInt256::parse(&value)
        .map_err(|_| CoreError::other(format!("Missing or invalid '{field}' field")))
}

/// Parses either a hex script hash or an address using the supplied protocol settings.
pub fn parse_script_hash_or_address(
    value: &str,
    protocol_settings: &ProtocolSettings,
) -> CoreResult<UInt160> {
    UInt160::parse(value)
        .map_err(|err| CoreError::other(err.to_string()))
        .or_else(|_| WalletHelper::to_script_hash(value, protocol_settings.address_version))
}

/// Reads a required script-hash-or-address field.
pub fn required_script_hash_or_address(
    json: &JObject,
    field: &str,
    protocol_settings: &ProtocolSettings,
    value_name: &str,
) -> CoreResult<UInt160> {
    let value = required_string(json, field).map_err(|e| CoreError::other(e.to_string()))?;
    parse_script_hash_or_address(&value, protocol_settings)
        .map_err(|_| CoreError::other(format!("Invalid {value_name}: {value}")))
}

/// Reads an optional script-hash-or-address field, preserving lossy legacy parsing.
pub fn optional_script_hash_or_address_lossy(
    json: &JObject,
    field: &str,
    protocol_settings: &ProtocolSettings,
) -> Option<UInt160> {
    optional_string(json, field)
        .as_deref()
        .and_then(|value| parse_script_hash_or_address(value, protocol_settings).ok())
}

/// Reads a required parent address field, preserving the RPC model's legacy address parsing.
pub fn required_address_script_hash(
    json: &JObject,
    field: &str,
    protocol_settings: &ProtocolSettings,
) -> CoreResult<UInt160> {
    let address = required_string(json, field).map_err(|e| CoreError::other(e.to_string()))?;
    if strip_hex_prefix(&address) != address.as_str() {
        UInt160::parse(&address)
            .map_err(|_| CoreError::other(format!("Invalid address: {address}")))
    } else {
        WalletHelper::to_script_hash(&address, protocol_settings.address_version)
            .map_err(|err| CoreError::other(format!("Invalid address: {err}")))
    }
}

/// Builds an ordered JSON object array token.
pub fn object_array<T>(items: &[T], to_object: impl FnMut(&T) -> JObject) -> JToken {
    object_array_from_iter(items.iter().map(to_object))
}

/// Builds an ordered JSON object array token from an object iterator.
pub fn object_array_from_iter(objects: impl IntoIterator<Item = JObject>) -> JToken {
    JToken::Array(JArray::from(
        objects.into_iter().map(JToken::Object).collect::<Vec<_>>(),
    ))
}

/// Builds an ordered JSON object array token from a fallible object mapper.
pub fn fallible_object_array<T, E>(
    items: &[T],
    to_object: impl FnMut(&T) -> Result<JObject, E>,
) -> Result<JToken, E> {
    let objects = items.iter().map(to_object).collect::<Result<Vec<_>, E>>()?;
    Ok(object_array_from_iter(objects))
}

/// Builds an ordered JSON array token.
pub fn token_array<T>(items: &[T], to_token: impl FnMut(&T) -> JToken) -> JToken {
    JToken::Array(JArray::from(items.iter().map(to_token).collect::<Vec<_>>()))
}

/// Builds an ordered JSON array token by cloning existing tokens.
pub fn cloned_token_array(items: &[JToken]) -> JToken {
    token_array(items, Clone::clone)
}

/// Builds an empty JSON array token.
pub fn empty_array() -> JToken {
    JToken::Array(JArray::new())
}

/// Parses a string array while preserving the RPC client's historical lossy behavior.
pub fn parse_string_array_lossy(json: &JObject, field: &str) -> Vec<String> {
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

/// Parses a `UInt256` string array while preserving the RPC client's historical lossy behavior.
pub fn parse_uint256_array_lossy(json: &JObject, field: &str) -> Vec<UInt256> {
    parse_string_array_lossy(json, field)
        .into_iter()
        .filter_map(|value| UInt256::parse(&value).ok())
        .collect()
}

/// Parses a JSON object array while preserving the RPC client's historical lossy behavior.
pub fn parse_object_array_lossy<T, E>(
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
                .filter_map(|obj| parse(obj).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Parses present entries from an optional JSON array.
///
/// Missing and non-array fields become an empty vector. Internal `None` slots
/// are skipped, while any present token that fails `parse` aborts the result.
pub fn parse_optional_present_token_array_strict<T>(
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

/// Parses an optional JSON array where every slot must contain a token.
///
/// Missing and non-array fields become an empty vector. Internal `None` slots
/// abort with `entry_error`, and any present token that fails `parse` aborts the
/// result.
pub fn parse_optional_token_array_strict<T>(
    json: &JObject,
    field: &str,
    entry_error: &str,
    mut parse: impl FnMut(&JToken) -> CoreResult<T>,
) -> CoreResult<Vec<T>> {
    json.get(field)
        .and_then(JToken::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    let token = entry
                        .as_ref()
                        .ok_or_else(|| CoreError::other(entry_error))?;
                    parse(token)
                })
                .collect()
        })
        .unwrap_or_else(|| Ok(Vec::new()))
}

/// Parses an optional string array where every slot must contain a string token.
pub fn parse_optional_string_array_strict(
    json: &JObject,
    field: &str,
    entry_error: &str,
) -> CoreResult<Vec<String>> {
    json.get(field)
        .and_then(JToken::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    entry
                        .as_ref()
                        .and_then(JToken::as_string)
                        .ok_or_else(|| CoreError::other(entry_error))
                })
                .collect()
        })
        .unwrap_or_else(|| Ok(Vec::new()))
}

/// Parses a base64-encoded string token.
pub fn parse_base64_token(token: &JToken, field: &str) -> CoreResult<Vec<u8>> {
    let text = token
        .as_string()
        .ok_or_else(|| CoreError::other(format!("Field '{field}' must be a base64 string")))?;
    general_purpose::STANDARD
        .decode(text.as_bytes())
        .map_err(|err| CoreError::other(format!("Invalid base64 data in '{field}': {err}")))
}

/// Builds a base64 string token.
pub(crate) fn base64_string_token(data: impl AsRef<[u8]>) -> JToken {
    JToken::String(general_purpose::STANDARD.encode(data.as_ref()))
}

/// Reads a base64 field while preserving lossy RPC model parsing.
pub(crate) fn optional_base64_field_lossy(json: &JObject, field: &str) -> Option<Vec<u8>> {
    json.get(field)
        .and_then(JToken::as_string)
        .and_then(|text| general_purpose::STANDARD.decode(text).ok())
}

/// Parses a u32 token that may be encoded as number or string.
pub fn parse_u32_token(token: &JToken, field: &str) -> CoreResult<u32> {
    parse_integer_token(token, field, "unsigned", |number| number as u32)
}

/// Parses a u64 token that may be encoded as number or string.
pub fn parse_u64_token(token: &JToken, field: &str) -> CoreResult<u64> {
    parse_integer_token(token, field, "unsigned", |number| number as u64)
}

/// Parses an i64 token that may be encoded as number or string.
pub fn parse_i64_token(token: &JToken, field: &str) -> CoreResult<i64> {
    parse_integer_token(token, field, "signed", |number| number as i64)
}

fn parse_integer_token<T>(
    token: &JToken,
    field: &str,
    integer_kind: &str,
    from_number: impl FnOnce(f64) -> T,
) -> CoreResult<T>
where
    T: FromStr,
    T::Err: Display,
{
    // String FIRST: Neo serializes large integers (e.g. sysfee/netfee) as
    // decimal strings, which must parse losslessly via FromStr. Going through
    // `as_number()` first would funnel them through f64 and silently corrupt any
    // value above 2^53.
    if let Some(text) = token.as_string() {
        text.parse::<T>().map_err(|err| {
            CoreError::other(format!(
                "Invalid {integer_kind} integer for '{field}': {err}"
            ))
        })
    } else if let JToken::Number(number) = token {
        let number = *number;
        if !number.is_finite() || number.fract() != 0.0 {
            return Err(CoreError::other(format!(
                "Invalid {integer_kind} integer for '{field}': {number} is not an integer"
            )));
        }
        // Round-trip through a decimal string so out-of-range values error
        // instead of saturating, with per-type bounds checking for free.
        format!("{number:.0}").parse::<T>().map_err(|err| {
            CoreError::other(format!(
                "Invalid {integer_kind} integer for '{field}': {err}"
            ))
        })
    } else if let Some(number) = token.as_number() {
        // Boolean coercion fallback (true -> 1.0 / false -> 0.0): prior behavior.
        Ok(from_number(number))
    } else {
        Err(CoreError::other(format!(
            "Field '{field}' must be a number"
        )))
    }
}

/// Parses a nonce token that may be a hex string or number.
pub fn parse_nonce_token(token: &JToken) -> CoreResult<u64> {
    if let Some(text) = token.as_string() {
        let value = strip_hex_prefix(&text);
        u64::from_str_radix(value, 16)
            .map_err(|err| CoreError::other(format!("Invalid nonce hex string '{text}': {err}")))
    } else if let Some(number) = token.as_number() {
        // 2^64 exclusive: `u64::MAX as f64` rounds UP to 2^64, so an exact 2^64
        // would otherwise slip through to a saturating cast.
        if !number.is_finite() || number < 0.0 || number.fract() != 0.0 || number >= 2f64.powi(64) {
            return Err(CoreError::other(format!(
                "Invalid nonce number '{number}': out of u64 range or not an integer"
            )));
        }
        Ok(number as u64)
    } else {
        Err(CoreError::other(
            "Nonce value must be a hex string or number",
        ))
    }
}

/// Parses an oracle response code supporting string, hex, or numeric values.
pub fn parse_oracle_response_code(token: &JToken) -> CoreResult<OracleResponseCode> {
    if let Some(text) = token.as_string() {
        match text.as_str() {
            "Success" => Ok(OracleResponseCode::Success),
            "ProtocolNotSupported" => Ok(OracleResponseCode::ProtocolNotSupported),
            "ConsensusUnreachable" => Ok(OracleResponseCode::ConsensusUnreachable),
            "NotFound" => Ok(OracleResponseCode::NotFound),
            "Timeout" => Ok(OracleResponseCode::Timeout),
            "Forbidden" => Ok(OracleResponseCode::Forbidden),
            "ResponseTooLarge" => Ok(OracleResponseCode::ResponseTooLarge),
            "InsufficientFunds" => Ok(OracleResponseCode::InsufficientFunds),
            "ContentTypeNotSupported" => Ok(OracleResponseCode::ContentTypeNotSupported),
            "Error" => Ok(OracleResponseCode::Error),
            other => {
                let normalized = strip_hex_prefix(other);
                let value = u8::from_str_radix(normalized, 16).map_err(|err| {
                    CoreError::other(format!("Invalid oracle response code '{other}': {err}"))
                })?;
                OracleResponseCode::from_byte(value).ok_or_else(|| {
                    CoreError::other(format!(
                        "Unknown oracle response code value '{other}' in RPC payload"
                    ))
                })
            }
        }
    } else if let Some(number) = token.as_number() {
        let value = number as u8;
        OracleResponseCode::from_byte(value).ok_or_else(|| {
            CoreError::other(format!(
                "Unknown oracle response code value '{value}' in RPC payload"
            ))
        })
    } else {
        Err(CoreError::other(
            "OracleResponse attribute 'code' must be a string or number",
        ))
    }
}

/// Converts an oracle response code to its RPC string representation.
pub const fn oracle_response_code_to_str(code: OracleResponseCode) -> &'static str {
    match code {
        OracleResponseCode::Success => "Success",
        OracleResponseCode::ProtocolNotSupported => "ProtocolNotSupported",
        OracleResponseCode::ConsensusUnreachable => "ConsensusUnreachable",
        OracleResponseCode::NotFound => "NotFound",
        OracleResponseCode::Timeout => "Timeout",
        OracleResponseCode::Forbidden => "Forbidden",
        OracleResponseCode::ResponseTooLarge => "ResponseTooLarge",
        OracleResponseCode::InsufficientFunds => "InsufficientFunds",
        OracleResponseCode::ContentTypeNotSupported => "ContentTypeNotSupported",
        OracleResponseCode::Error => "Error",
    }
}

pub fn jtoken_to_serde(token: &JToken) -> CoreResult<JsonValue> {
    serde_json::from_str(&token.to_string()).map_err(|err| CoreError::other(err.to_string()))
}

#[cfg(test)]
#[path = "../../tests/client/utility/parsing.rs"]
mod tests;
