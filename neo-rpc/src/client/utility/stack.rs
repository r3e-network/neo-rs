use crate::client::models::RpcStackItem;
use base64::{Engine as _, engine::general_purpose};
use neo_error::{CoreError, CoreResult};
use neo_serialization::json::{JObject, JToken};
use num_bigint::BigInt;
use num_traits::Zero;
use thiserror::Error;

use super::parsing::{fallible_object_array, parse_base64_token, parse_u32_token};
use crate::types::json::parse_object_array_lossy;

/// Error type for JSON-RPC stack-item parsing operations.
///
/// Replaces the previous `Result<_, String>` returns across the 7
/// `stack.rs` call-sites. Includes `From<String>`/`From<&str>` for
/// backward compatibility with `.ok_or(format!(...))?` patterns.
#[derive(Debug, Error)]
pub enum StackParseError {
    /// Required field is missing from the JSON object.
    #[error("{0}")]
    MissingField(String),

    /// A field had the wrong JSON type.
    #[error("{0}")]
    InvalidType(String),

    /// A field value could not be parsed (e.g. invalid integer, base64).
    #[error("{0}")]
    InvalidValue(String),
}

impl From<String> for StackParseError {
    fn from(message: String) -> Self {
        // Bucket legacy strings into the closest matching variant.
        if message.contains("missing '") || message.contains("missing '") {
            return Self::MissingField(message);
        }
        if message.contains("must be") || message.contains("type must") {
            return Self::InvalidType(message);
        }
        Self::InvalidValue(message)
    }
}

impl From<&str> for StackParseError {
    fn from(message: &str) -> Self {
        Self::from(message.to_string())
    }
}

impl From<StackParseError> for String {
    fn from(err: StackParseError) -> Self {
        err.to_string()
    }
}

impl From<CoreError> for StackParseError {
    fn from(err: CoreError) -> Self {
        Self::InvalidValue(err.to_string())
    }
}

/// Converts a JSON-RPC stack item into an immutable transport DTO.
pub fn stack_item_from_json(json: &JObject) -> Result<RpcStackItem, StackParseError> {
    let item_type = json
        .get("type")
        .and_then(neo_serialization::json::JToken::as_string)
        .ok_or_else(|| {
            StackParseError::MissingField("StackItem entry missing 'type' field".to_string())
        })?;

    match item_type.as_str() {
        "Any" => Ok(fallback_text_or_null(json)),
        "Boolean" => {
            let value = json
                .get("value")
                .map(neo_serialization::json::JToken::as_boolean)
                .ok_or_else(|| {
                    StackParseError::MissingField(
                        "Boolean stack item missing 'value' field".to_string(),
                    )
                })?;
            Ok(RpcStackItem::Boolean(value))
        }
        "Integer" => {
            let value_token = json.get("value").ok_or_else(|| {
                StackParseError::MissingField(
                    "Integer stack item missing 'value' field".to_string(),
                )
            })?;
            let text = value_token.as_string().ok_or_else(|| {
                StackParseError::InvalidType(
                    "Integer stack item value must be a string".to_string(),
                )
            })?;
            let integer = BigInt::parse_bytes(text.as_bytes(), 10).ok_or_else(|| {
                StackParseError::InvalidValue(format!("Invalid integer stack item value: {text}"))
            })?;
            Ok(RpcStackItem::Integer(integer))
        }
        "ByteString" => parse_base64_stack_item(json, "ByteString", RpcStackItem::ByteString),
        "Buffer" => parse_base64_stack_item(json, "Buffer", RpcStackItem::Buffer),
        "Array" => parse_stack_sequence(json, "Array", RpcStackItem::Array),
        "Struct" => parse_stack_sequence(json, "Struct", RpcStackItem::Struct),
        "Map" => {
            let values = json
                .get("value")
                .and_then(|token| token.as_array())
                .ok_or_else(|| {
                    StackParseError::MissingField(
                        "Map stack item missing 'value' array".to_string(),
                    )
                })?;
            let mut entries = Vec::with_capacity(values.len());
            for entry in values.children() {
                let token = entry.as_ref().ok_or_else(|| {
                    StackParseError::InvalidType("Map entries must be objects".to_string())
                })?;
                let obj = token.as_object().ok_or_else(|| {
                    StackParseError::InvalidType("Map entries must be objects".to_string())
                })?;
                let key_obj = obj
                    .get("key")
                    .and_then(|token| token.as_object())
                    .ok_or("Map entry missing 'key' object")?;
                let value_obj = obj
                    .get("value")
                    .and_then(|token| token.as_object())
                    .ok_or("Map entry missing 'value' object")?;
                entries.push((
                    stack_item_from_json(key_obj)?,
                    stack_item_from_json(value_obj)?,
                ));
            }
            Ok(RpcStackItem::Map(entries))
        }
        "Pointer" => {
            let index_token = json
                .get("value")
                .ok_or("Pointer stack item missing 'value' field")?;
            Ok(RpcStackItem::Pointer(
                parse_u32_token(index_token, "value").map_err(StackParseError::from)?,
            ))
        }
        "InteropInterface" => Ok(RpcStackItem::InteropInterface {
            interface: optional_text_field(json, "interface"),
            id: optional_text_field(json, "id"),
        }),
        _other => Ok(fallback_text_or_null(json)),
    }
}

/// Converts an RPC stack item DTO into its JSON representation.
pub fn stack_item_to_json(item: &RpcStackItem) -> CoreResult<JObject> {
    let mut json = JObject::new();
    json.insert(
        "type".to_string(),
        JToken::String(rpc_stack_item_type_name(item).to_string()),
    );

    match item {
        RpcStackItem::Null => {}
        RpcStackItem::InteropInterface { interface, id } => {
            if let Some(interface) = interface {
                json.insert("interface".to_string(), JToken::String(interface.clone()));
            }
            if let Some(id) = id {
                json.insert("id".to_string(), JToken::String(id.clone()));
            }
        }
        RpcStackItem::Boolean(value) => {
            json.insert("value".to_string(), JToken::Boolean(*value));
        }
        RpcStackItem::Integer(value) => {
            json.insert("value".to_string(), JToken::String(value.to_string()));
        }
        RpcStackItem::ByteString(bytes) => {
            insert_base64_value(&mut json, bytes);
        }
        RpcStackItem::Buffer(bytes) => {
            insert_base64_value(&mut json, bytes);
        }
        RpcStackItem::Pointer(index) => {
            json.insert("value".to_string(), JToken::Number(f64::from(*index)));
        }
        RpcStackItem::Array(items) | RpcStackItem::Struct(items) => {
            json.insert("value".to_string(), stack_items_to_json(items)?);
        }
        RpcStackItem::Map(entries) => {
            json.insert(
                "value".to_string(),
                fallible_object_array(entries, |entry| {
                    let (key, value) = entry;
                    let mut entry = JObject::new();
                    entry.insert("key".to_string(), JToken::Object(stack_item_to_json(key)?));
                    entry.insert(
                        "value".to_string(),
                        JToken::Object(stack_item_to_json(value)?),
                    );
                    Ok::<_, CoreError>(entry)
                })?,
            );
        }
    }

    Ok(json)
}

pub fn stack_items_to_json(items: &[RpcStackItem]) -> CoreResult<JToken> {
    fallible_object_array(items, stack_item_to_json)
}

pub fn stack_items_from_json_field(json: &JObject, field: &str) -> Vec<RpcStackItem> {
    parse_object_array_lossy(json, field, stack_item_from_json)
}

fn fallback_text_or_null(json: &JObject) -> RpcStackItem {
    let value = json.get("value");
    let text = value
        .and_then(|token| token.as_string())
        .or_else(|| value.map(std::string::ToString::to_string));

    if let Some(text) = text {
        RpcStackItem::ByteString(text.into_bytes())
    } else {
        RpcStackItem::Null
    }
}

fn optional_text_field(json: &JObject, field: &str) -> Option<String> {
    json.get(field)
        .and_then(|token| token.as_string())
        .or_else(|| {
            json.get(field)
                .and_then(|token| (!matches!(token, JToken::Null)).then(|| token.to_string()))
        })
}

fn parse_base64_stack_item(
    json: &JObject,
    type_name: &str,
    make_value: impl FnOnce(Vec<u8>) -> RpcStackItem,
) -> Result<RpcStackItem, StackParseError> {
    let value_token = json.get("value").ok_or_else(|| {
        StackParseError::MissingField(format!("{type_name} stack item missing 'value' field"))
    })?;
    let data = parse_base64_token(value_token, "value").map_err(StackParseError::from)?;
    Ok(make_value(data))
}

fn parse_stack_sequence(
    json: &JObject,
    type_name: &str,
    make_value: impl FnOnce(Vec<RpcStackItem>) -> RpcStackItem,
) -> Result<RpcStackItem, StackParseError> {
    let values = json
        .get("value")
        .and_then(|token| token.as_array())
        .ok_or_else(|| {
            StackParseError::MissingField(format!("{type_name} stack item missing 'value' array"))
        })?;
    let mut items = Vec::with_capacity(values.len());
    for value in values.children() {
        let token = value.as_ref().ok_or_else(|| {
            StackParseError::InvalidType(format!("{type_name} entries must be objects"))
        })?;
        let obj = token.as_object().ok_or_else(|| {
            StackParseError::InvalidType(format!("{type_name} entries must be objects"))
        })?;
        items.push(stack_item_from_json(obj)?);
    }
    Ok(make_value(items))
}

fn insert_base64_value(json: &mut JObject, bytes: &[u8]) {
    json.insert(
        "value".to_string(),
        JToken::String(general_purpose::STANDARD.encode(bytes)),
    );
}

pub fn rpc_stack_item_to_bigint(value: &RpcStackItem) -> CoreResult<BigInt> {
    match value {
        RpcStackItem::Boolean(value) => Ok(BigInt::from(if *value { 1 } else { 0 })),
        RpcStackItem::Integer(value) => Ok(value.clone()),
        RpcStackItem::ByteString(bytes) | RpcStackItem::Buffer(bytes) => {
            Ok(BigInt::from_signed_bytes_le(bytes))
        }
        RpcStackItem::Null => Err(CoreError::other("Cannot convert Null to Integer")),
        RpcStackItem::Array(..)
        | RpcStackItem::Struct(..)
        | RpcStackItem::Map(..)
        | RpcStackItem::InteropInterface { .. }
        | RpcStackItem::Pointer(_) => Err(CoreError::other("Cannot convert to Integer")),
    }
}

pub fn rpc_stack_item_to_bool(value: &RpcStackItem) -> bool {
    match value {
        RpcStackItem::Null => false,
        RpcStackItem::Boolean(value) => *value,
        RpcStackItem::Integer(value) => !value.is_zero(),
        RpcStackItem::ByteString(bytes) => bytes.iter().any(|byte| *byte != 0),
        RpcStackItem::Buffer(_)
        | RpcStackItem::Array(_)
        | RpcStackItem::Struct(_)
        | RpcStackItem::Map(_)
        | RpcStackItem::Pointer(_)
        | RpcStackItem::InteropInterface { .. } => true,
    }
}

pub fn rpc_stack_item_to_string(value: &RpcStackItem) -> CoreResult<String> {
    match value {
        RpcStackItem::ByteString(bytes) | RpcStackItem::Buffer(bytes) => {
            String::from_utf8(bytes.clone()).map_err(|err| CoreError::other(err.to_string()))
        }
        RpcStackItem::Integer(value) => Ok(value.to_string()),
        RpcStackItem::Boolean(value) => Ok(value.to_string()),
        _ => Err(CoreError::other(
            "Unsupported stack item for string conversion",
        )),
    }
}

fn rpc_stack_item_type_name(item: &RpcStackItem) -> &'static str {
    match item {
        RpcStackItem::Null => "Any",
        RpcStackItem::Boolean(_) => "Boolean",
        RpcStackItem::Integer(_) => "Integer",
        RpcStackItem::ByteString(_) => "ByteString",
        RpcStackItem::Buffer(..) => "Buffer",
        RpcStackItem::Array(..) => "Array",
        RpcStackItem::Struct(..) => "Struct",
        RpcStackItem::Map(..) => "Map",
        RpcStackItem::InteropInterface { .. } => "InteropInterface",
        RpcStackItem::Pointer(_) => "Pointer",
    }
}
