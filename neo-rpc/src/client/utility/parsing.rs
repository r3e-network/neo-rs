use base64::{Engine as _, engine::general_purpose};
use neo_config::ProtocolSettings;
use neo_core::network::payloads::oracle_response_code::OracleResponseCode;
use neo_core::wallets::helper::Helper as WalletHelper;
use neo_json::{JArray, JObject, JToken};
use neo_primitives::{UInt160, UInt256};
use num_bigint::BigInt;
use serde_json::Value as JsonValue;
use std::str::FromStr;

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

/// Reads a required numeric field as a `u64`.
pub fn required_u64_number(json: &JObject, field: &str) -> Result<u64, String> {
    json.get(field)
        .and_then(JToken::as_number)
        .map(|value| value as u64)
        .ok_or_else(|| format!("Missing or invalid '{field}' field"))
}

/// Reads a required numeric field as a `u32`.
pub fn required_u32_number(json: &JObject, field: &str) -> Result<u32, String> {
    json.get(field)
        .and_then(JToken::as_number)
        .map(|value| value as u32)
        .ok_or_else(|| format!("Missing or invalid '{field}' field"))
}

/// Reads a required numeric field as a `u16`.
pub fn required_u16_number(json: &JObject, field: &str) -> Result<u16, String> {
    json.get(field)
        .and_then(JToken::as_number)
        .map(|value| value as u16)
        .ok_or_else(|| format!("Missing or invalid '{field}' field"))
}

/// Reads a required decimal integer string field as a `BigInt`.
pub fn required_bigint_string(
    json: &JObject,
    field: &str,
    value_name: &str,
) -> Result<BigInt, String> {
    let value = required_string(json, field)?;
    BigInt::from_str(&value).map_err(|_| format!("Invalid {value_name}: {value}"))
}

/// Reads a required `UInt256` string field.
pub fn required_uint256(json: &JObject, field: &str) -> Result<UInt256, String> {
    let value = required_string(json, field)?;
    UInt256::parse(&value).map_err(|_| format!("Missing or invalid '{field}' field"))
}

/// Parses either a hex script hash or an address using the supplied protocol settings.
pub fn parse_script_hash_or_address(
    value: &str,
    protocol_settings: &ProtocolSettings,
) -> Result<UInt160, String> {
    UInt160::parse(value)
        .map_err(|err| err.to_string())
        .or_else(|_| WalletHelper::to_script_hash(value, protocol_settings.address_version))
}

/// Reads a required script-hash-or-address field.
pub fn required_script_hash_or_address(
    json: &JObject,
    field: &str,
    protocol_settings: &ProtocolSettings,
    value_name: &str,
) -> Result<UInt160, String> {
    let value = required_string(json, field)?;
    parse_script_hash_or_address(&value, protocol_settings)
        .map_err(|_| format!("Invalid {value_name}: {value}"))
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
) -> Result<UInt160, String> {
    let address = required_string(json, field)?;
    if address.starts_with("0x") {
        UInt160::parse(&address).map_err(|_| format!("Invalid address: {address}"))
    } else {
        WalletHelper::to_script_hash(&address, protocol_settings.address_version)
            .map_err(|err| format!("Invalid address: {err}"))
    }
}

/// Builds an ordered JSON object array token.
pub fn object_array<T>(items: &[T], mut to_object: impl FnMut(&T) -> JObject) -> JToken {
    let objects = items
        .iter()
        .map(|item| JToken::Object(to_object(item)))
        .collect::<Vec<_>>();
    JToken::Array(JArray::from(objects))
}

/// Builds an ordered JSON array token.
pub fn token_array<T>(items: &[T], to_token: impl FnMut(&T) -> JToken) -> JToken {
    JToken::Array(JArray::from(items.iter().map(to_token).collect::<Vec<_>>()))
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

/// Parses present entries from an optional JSON array.
///
/// Missing and non-array fields become an empty vector. Internal `None` slots
/// are skipped, while any present token that fails `parse` aborts the result.
pub fn parse_optional_present_token_array_strict<T>(
    json: &JObject,
    field: &str,
    mut parse: impl FnMut(&JToken) -> Result<T, String>,
) -> Result<Vec<T>, String> {
    json.get(field)
        .and_then(JToken::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.as_ref())
                .map(|token| parse(token))
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
    mut parse: impl FnMut(&JToken) -> Result<T, String>,
) -> Result<Vec<T>, String> {
    json.get(field)
        .and_then(JToken::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    let token = entry.as_ref().ok_or_else(|| entry_error.to_string())?;
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
) -> Result<Vec<String>, String> {
    json.get(field)
        .and_then(JToken::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    entry
                        .as_ref()
                        .and_then(JToken::as_string)
                        .ok_or_else(|| entry_error.to_string())
                })
                .collect()
        })
        .unwrap_or_else(|| Ok(Vec::new()))
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
    use neo_config::ProtocolSettings;
    use neo_core::wallets::helper::Helper as WalletHelper;
    use neo_json::{JArray, JObject};
    use neo_primitives::UInt160;

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

    #[test]
    fn script_hash_or_address_parsing_accepts_hex_and_address() {
        let settings = ProtocolSettings::default_settings();
        let hash = UInt160::zero();
        let address = WalletHelper::to_address(&hash, settings.address_version);

        assert_eq!(
            parse_script_hash_or_address(&hash.to_string(), &settings).unwrap(),
            hash
        );
        assert_eq!(
            parse_script_hash_or_address(&address, &settings).unwrap(),
            hash
        );
    }

    #[test]
    fn optional_script_hash_or_address_is_lossy() {
        let settings = ProtocolSettings::default_settings();
        let mut json = JObject::new();
        json.insert(
            "transferaddress".to_string(),
            JToken::String("not a valid address".to_string()),
        );

        assert_eq!(
            optional_script_hash_or_address_lossy(&json, "transferaddress", &settings),
            None
        );
    }

    #[test]
    fn required_address_script_hash_preserves_parent_address_semantics() {
        let settings = ProtocolSettings::default_settings();
        let hash = UInt160::zero();
        let address = WalletHelper::to_address(&hash, settings.address_version);

        let mut base58 = JObject::new();
        base58.insert("address".to_string(), JToken::String(address));
        assert_eq!(
            required_address_script_hash(&base58, "address", &settings).unwrap(),
            hash
        );

        let mut prefixed_hex = JObject::new();
        prefixed_hex.insert("address".to_string(), JToken::String(hash.to_string()));
        assert_eq!(
            required_address_script_hash(&prefixed_hex, "address", &settings).unwrap(),
            hash
        );

        let mut bare_hex = JObject::new();
        bare_hex.insert(
            "address".to_string(),
            JToken::String(hash.to_string().trim_start_matches("0x").to_string()),
        );
        assert!(required_address_script_hash(&bare_hex, "address", &settings).is_err());
    }

    #[test]
    fn object_array_preserves_item_order() {
        let values = ["first", "second"];
        let token = object_array(&values, |value| {
            let mut object = JObject::new();
            object.insert("value".to_string(), JToken::String((*value).to_string()));
            object
        });

        assert_eq!(
            token.to_string(),
            r#"[{"value":"first"},{"value":"second"}]"#
        );
    }

    #[test]
    fn token_array_preserves_item_order() {
        let values = ["first", "second"];
        let token = token_array(&values, |value| JToken::String((*value).to_string()));

        assert_eq!(token.to_string(), r#"["first","second"]"#);
    }

    #[test]
    fn string_array_lossy_keeps_only_strings() {
        let mut entries = JArray::new();
        entries.add(Some(JToken::String("first".to_string())));
        entries.add(None);
        entries.add(Some(JToken::Number(1.0)));
        entries.add(Some(JToken::String("second".to_string())));

        let mut root = JObject::new();
        root.insert("items".to_string(), JToken::Array(entries));

        assert_eq!(
            parse_string_array_lossy(&root, "items"),
            vec!["first".to_string(), "second".to_string()]
        );
        assert!(parse_string_array_lossy(&root, "missing").is_empty());
    }

    #[test]
    fn uint256_array_lossy_keeps_only_valid_hash_strings() {
        let mut entries = JArray::new();
        entries.add(Some(JToken::String(UInt256::zero().to_string())));
        entries.add(Some(JToken::String("not a hash".to_string())));
        entries.add(None);

        let mut root = JObject::new();
        root.insert("hashes".to_string(), JToken::Array(entries));

        assert_eq!(
            parse_uint256_array_lossy(&root, "hashes"),
            vec![UInt256::zero()]
        );
    }

    #[test]
    fn optional_present_token_array_strict_skips_empty_slots_and_errors_present_tokens() {
        let mut entries = JArray::new();
        entries.add(Some(JToken::Number(1.0)));
        entries.add(None);
        entries.add(Some(JToken::Number(2.0)));

        let mut root = JObject::new();
        root.insert("items".to_string(), JToken::Array(entries));

        let parsed = parse_optional_present_token_array_strict(&root, "items", |token| {
            token
                .as_number()
                .map(|value| value as u8)
                .ok_or_else(|| "entry must be a number".to_string())
        })
        .expect("strict present entries");
        assert_eq!(parsed, vec![1, 2]);
        assert!(
            parse_optional_present_token_array_strict(&root, "missing", |_| Ok::<_, String>(0))
                .expect("missing defaults")
                .is_empty()
        );
        let mut non_array = JObject::new();
        non_array.insert("items".to_string(), JToken::Boolean(true));
        assert!(
            parse_optional_present_token_array_strict(&non_array, "items", |_| {
                Ok::<_, String>(0)
            })
            .expect("non-array defaults")
            .is_empty()
        );

        let mut invalid = JObject::new();
        invalid.insert(
            "items".to_string(),
            JToken::Array(JArray::from(vec![JToken::String("bad".to_string())])),
        );
        assert_eq!(
            parse_optional_present_token_array_strict(&invalid, "items", |token| {
                token
                    .as_number()
                    .map(|value| value as u8)
                    .ok_or_else(|| "entry must be a number".to_string())
            })
            .expect_err("present invalid token errors"),
            "entry must be a number"
        );
    }

    #[test]
    fn optional_token_array_strict_errors_on_empty_or_invalid_slots() {
        let mut entries = JArray::new();
        entries.add(Some(JToken::Number(1.0)));
        entries.add(Some(JToken::Number(2.0)));

        let mut root = JObject::new();
        root.insert("items".to_string(), JToken::Array(entries));

        let parsed =
            parse_optional_token_array_strict(&root, "items", "entry must be a number", |token| {
                token
                    .as_number()
                    .map(|value| value as u8)
                    .ok_or_else(|| "entry must be a number".to_string())
            })
            .expect("strict tokens");
        assert_eq!(parsed, vec![1, 2]);
        assert!(
            parse_optional_token_array_strict(&root, "missing", "entry must be a number", |_| {
                Ok::<_, String>(0)
            })
            .expect("missing defaults")
            .is_empty()
        );
        let mut non_array = JObject::new();
        non_array.insert("items".to_string(), JToken::Boolean(true));
        assert!(
            parse_optional_token_array_strict(
                &non_array,
                "items",
                "entry must be a number",
                |_| { Ok::<_, String>(0) }
            )
            .expect("non-array defaults")
            .is_empty()
        );

        let mut missing_slot = JArray::new();
        missing_slot.add(None);
        let mut invalid = JObject::new();
        invalid.insert("items".to_string(), JToken::Array(missing_slot));
        assert_eq!(
            parse_optional_token_array_strict(
                &invalid,
                "items",
                "entry must be a number",
                |_| Ok::<_, String>(0)
            )
            .expect_err("empty slot errors"),
            "entry must be a number"
        );

        invalid.insert(
            "items".to_string(),
            JToken::Array(JArray::from(vec![JToken::String("bad".to_string())])),
        );
        assert_eq!(
            parse_optional_token_array_strict(
                &invalid,
                "items",
                "entry must be a number",
                |token| {
                    token
                        .as_number()
                        .map(|value| value as u8)
                        .ok_or_else(|| "entry parse failed".to_string())
                },
            )
            .expect_err("present invalid token errors"),
            "entry parse failed"
        );
    }

    #[test]
    fn optional_string_array_strict_errors_on_empty_or_non_string_slots() {
        let mut entries = JArray::new();
        entries.add(Some(JToken::String("first".to_string())));
        entries.add(Some(JToken::String("second".to_string())));

        let mut root = JObject::new();
        root.insert("items".to_string(), JToken::Array(entries));

        assert_eq!(
            parse_optional_string_array_strict(&root, "items", "entry must be a string")
                .expect("strict strings"),
            vec!["first".to_string(), "second".to_string()]
        );
        assert!(
            parse_optional_string_array_strict(&root, "missing", "entry must be a string")
                .expect("missing defaults")
                .is_empty()
        );
        let mut non_array = JObject::new();
        non_array.insert("items".to_string(), JToken::Boolean(true));
        assert!(
            parse_optional_string_array_strict(&non_array, "items", "entry must be a string")
                .expect("non-array defaults")
                .is_empty()
        );

        let mut missing_slot = JArray::new();
        missing_slot.add(None);
        let mut invalid = JObject::new();
        invalid.insert("items".to_string(), JToken::Array(missing_slot));
        assert_eq!(
            parse_optional_string_array_strict(&invalid, "items", "entry must be a string")
                .expect_err("empty slot errors"),
            "entry must be a string"
        );

        invalid.insert(
            "items".to_string(),
            JToken::Array(JArray::from(vec![JToken::Number(1.0)])),
        );
        assert_eq!(
            parse_optional_string_array_strict(&invalid, "items", "entry must be a string")
                .expect_err("non-string errors"),
            "entry must be a string"
        );
    }
}
