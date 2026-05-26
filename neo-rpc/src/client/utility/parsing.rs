use base64::{engine::general_purpose, Engine as _};
use neo_core::network::payloads::oracle_response_code::OracleResponseCode;
use neo_json::{JObject, JToken};
use serde_json::Value as JsonValue;

/// Reads a required string field from a JSON object.
pub fn required_string(json: &JObject, field: &str) -> Result<String, String> {
    json.get(field)
        .and_then(JToken::as_string)
        .ok_or_else(|| format!("Missing or invalid '{field}' field"))
}

/// Reads an optional string field from a JSON object.
pub fn optional_string(json: &JObject, field: &str) -> Option<String> {
    json.get(field).and_then(JToken::as_string)
}

/// Parses a JSON object array while preserving the RPC client's historical lossy behavior.
pub fn parse_object_array_lossy<T>(
    json: &JObject,
    field: &str,
    mut parse: impl FnMut(&JObject) -> Result<T, String>,
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

/// Parses a base64-encoded string token.
pub fn parse_base64_token(token: &JToken, field: &str) -> Result<Vec<u8>, String> {
    let text = token
        .as_string()
        .ok_or_else(|| format!("Field '{field}' must be a base64 string"))?;
    general_purpose::STANDARD
        .decode(text.as_bytes())
        .map_err(|err| format!("Invalid base64 data in '{field}': {err}"))
}

/// Parses a u32 token that may be encoded as number or string.
pub fn parse_u32_token(token: &JToken, field: &str) -> Result<u32, String> {
    if let Some(number) = token.as_number() {
        Ok(number as u32)
    } else if let Some(text) = token.as_string() {
        text.parse::<u32>()
            .map_err(|err| format!("Invalid unsigned integer for '{field}': {err}"))
    } else {
        Err(format!("Field '{field}' must be a number"))
    }
}

/// Parses a u64 token that may be encoded as number or string.
pub fn parse_u64_token(token: &JToken, field: &str) -> Result<u64, String> {
    if let Some(number) = token.as_number() {
        Ok(number as u64)
    } else if let Some(text) = token.as_string() {
        text.parse::<u64>()
            .map_err(|err| format!("Invalid unsigned integer for '{field}': {err}"))
    } else {
        Err(format!("Field '{field}' must be a number"))
    }
}

/// Parses an i64 token that may be encoded as number or string.
pub fn parse_i64_token(token: &JToken, field: &str) -> Result<i64, String> {
    if let Some(number) = token.as_number() {
        Ok(number as i64)
    } else if let Some(text) = token.as_string() {
        text.parse::<i64>()
            .map_err(|err| format!("Invalid signed integer for '{field}': {err}"))
    } else {
        Err(format!("Field '{field}' must be a number"))
    }
}

/// Parses a nonce token that may be a hex string or number.
pub fn parse_nonce_token(token: &JToken) -> Result<u64, String> {
    if let Some(text) = token.as_string() {
        let value = text.trim_start_matches("0x");
        u64::from_str_radix(value, 16)
            .map_err(|err| format!("Invalid nonce hex string '{text}': {err}"))
    } else if let Some(number) = token.as_number() {
        Ok(number as u64)
    } else {
        Err("Nonce value must be a hex string or number".to_string())
    }
}

/// Parses an oracle response code supporting string, hex, or numeric values.
pub fn parse_oracle_response_code(
    token: &JToken,
) -> Result<neo_core::network::payloads::oracle_response_code::OracleResponseCode, String> {
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
                let normalized = other.trim_start_matches("0x");
                let value = u8::from_str_radix(normalized, 16)
                    .map_err(|err| format!("Invalid oracle response code '{other}': {err}"))?;
                OracleResponseCode::from_byte(value).ok_or_else(|| {
                    format!("Unknown oracle response code value '{other}' in RPC payload")
                })
            }
        }
    } else if let Some(number) = token.as_number() {
        let value = number as u8;
        OracleResponseCode::from_byte(value)
            .ok_or_else(|| format!("Unknown oracle response code value '{value}' in RPC payload"))
    } else {
        Err("OracleResponse attribute 'code' must be a string or number".to_string())
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

pub fn jtoken_to_serde(token: &JToken) -> Result<JsonValue, String> {
    serde_json::from_str(&token.to_string()).map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_json::{JArray, JObject};

    #[test]
    fn object_array_lossy_keeps_only_successful_objects() {
        let mut valid = JObject::new();
        valid.insert("value".to_string(), JToken::String("ok".to_string()));

        let mut invalid = JObject::new();
        invalid.insert("value".to_string(), JToken::String("skip".to_string()));

        let mut entries = JArray::new();
        entries.add(Some(JToken::Object(valid)));
        entries.add(None);
        entries.add(Some(JToken::String("not an object".to_string())));
        entries.add(Some(JToken::Object(invalid)));

        let mut root = JObject::new();
        root.insert("items".to_string(), JToken::Array(entries));

        let parsed = parse_object_array_lossy(&root, "items", |obj| {
            let value = obj.get("value").and_then(JToken::as_string).unwrap();
            if value == "ok" {
                Ok(value)
            } else {
                Err("skip".to_string())
            }
        });

        assert_eq!(parsed, vec!["ok".to_string()]);
        let missing = parse_object_array_lossy(&root, "missing", |_| Ok::<_, String>("unused"));
        assert!(missing.is_empty());
    }
}
