use base64::{engine::general_purpose, Engine as _};
use neo_json::{JObject, JToken};
use neo_vm_rs::StackValue;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

use super::parsing::{
    fallible_object_array, parse_base64_token, parse_object_array_lossy, parse_u32_token,
};

/// Converts a `neo-json` representation of an RPC stack item into `neo-vm-rs`.
pub fn stack_item_from_json(json: &JObject) -> Result<StackValue, String> {
    let item_type = json
        .get("type")
        .and_then(neo_json::JToken::as_string)
        .ok_or("StackItem entry missing 'type' field")?;

    match item_type.as_str() {
        "Any" => Ok(fallback_text_or_null(json)),
        "Boolean" => {
            let value = json
                .get("value")
                .map(neo_json::JToken::as_boolean)
                .ok_or("Boolean stack item missing 'value' field")?;
            Ok(StackValue::Boolean(value))
        }
        "Integer" => {
            let value_token = json
                .get("value")
                .ok_or("Integer stack item missing 'value' field")?;
            let text = value_token
                .as_string()
                .ok_or("Integer stack item value must be a string")?;
            let integer = BigInt::parse_bytes(text.as_bytes(), 10)
                .ok_or("Invalid integer stack item value")?;
            Ok(integer_stack_value(integer))
        }
        "ByteString" => parse_base64_stack_value(json, "ByteString", StackValue::ByteString),
        "Buffer" => parse_base64_stack_value(json, "Buffer", StackValue::Buffer),
        "Array" => parse_stack_sequence(json, "Array", StackValue::Array),
        "Struct" => parse_stack_sequence(json, "Struct", StackValue::Struct),
        "Map" => {
            let values = json
                .get("value")
                .and_then(|token| token.as_array())
                .ok_or("Map stack item missing 'value' array")?;
            let mut entries = Vec::with_capacity(values.len());
            for entry in values.children() {
                let token = entry.as_ref().ok_or("Map entries must be objects")?;
                let obj = token.as_object().ok_or("Map entries must be objects")?;
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
            Ok(StackValue::Map(entries))
        }
        "Pointer" => {
            let index_token = json
                .get("value")
                .ok_or("Pointer stack item missing 'value' field")?;
            Ok(StackValue::Pointer(i64::from(parse_u32_token(
                index_token,
                "value",
            )?)))
        }
        "InteropInterface" => Ok(StackValue::Interop(0)),
        _other => Ok(fallback_text_or_null(json)),
    }
}

/// Converts an RPC stack value into its `neo-json` representation.
pub fn stack_item_to_json(item: &StackValue) -> Result<JObject, String> {
    let mut json = JObject::new();
    json.insert(
        "type".to_string(),
        JToken::String(stack_value_type_name(item).to_string()),
    );

    match item {
        StackValue::Null | StackValue::Interop(_) | StackValue::Iterator(_) => {}
        StackValue::Boolean(value) => {
            json.insert("value".to_string(), JToken::Boolean(*value));
        }
        StackValue::Integer(value) => {
            json.insert("value".to_string(), JToken::String(value.to_string()));
        }
        StackValue::BigInteger(bytes) => {
            json.insert(
                "value".to_string(),
                JToken::String(BigInt::from_signed_bytes_le(bytes).to_string()),
            );
        }
        StackValue::ByteString(bytes) => {
            insert_base64_value(&mut json, bytes);
        }
        StackValue::Buffer(bytes) => {
            insert_base64_value(&mut json, bytes);
        }
        StackValue::Pointer(index) => {
            json.insert("value".to_string(), JToken::Number(*index as f64));
        }
        StackValue::Array(items) | StackValue::Struct(items) => {
            json.insert("value".to_string(), stack_items_to_json(items)?);
        }
        StackValue::Map(entries) => {
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
                    Ok::<_, String>(entry)
                })?,
            );
        }
    }

    Ok(json)
}

pub fn stack_items_to_json(items: &[StackValue]) -> Result<JToken, String> {
    fallible_object_array(items, stack_item_to_json)
}

pub fn stack_items_from_json_field(json: &JObject, field: &str) -> Vec<StackValue> {
    parse_object_array_lossy(json, field, stack_item_from_json)
}

fn fallback_text_or_null(json: &JObject) -> StackValue {
    let value = json.get("value");
    let text = value
        .and_then(|token| token.as_string())
        .or_else(|| value.map(std::string::ToString::to_string));

    if let Some(text) = text {
        StackValue::ByteString(text.into_bytes())
    } else {
        StackValue::Null
    }
}

fn parse_base64_stack_value(
    json: &JObject,
    type_name: &str,
    make_value: impl FnOnce(Vec<u8>) -> StackValue,
) -> Result<StackValue, String> {
    let value_token = json
        .get("value")
        .ok_or_else(|| format!("{type_name} stack item missing 'value' field"))?;
    let data = parse_base64_token(value_token, "value")?;
    Ok(make_value(data))
}

fn parse_stack_sequence(
    json: &JObject,
    type_name: &str,
    make_value: impl FnOnce(Vec<StackValue>) -> StackValue,
) -> Result<StackValue, String> {
    let values = json
        .get("value")
        .and_then(|token| token.as_array())
        .ok_or_else(|| format!("{type_name} stack item missing 'value' array"))?;
    let mut items = Vec::with_capacity(values.len());
    for value in values.children() {
        let token = value
            .as_ref()
            .ok_or_else(|| format!("{type_name} entries must be objects"))?;
        let obj = token
            .as_object()
            .ok_or_else(|| format!("{type_name} entries must be objects"))?;
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

pub fn stack_value_to_bigint(value: &StackValue) -> Result<BigInt, String> {
    match value {
        StackValue::Boolean(value) => Ok(BigInt::from(if *value { 1 } else { 0 })),
        StackValue::Integer(value) => Ok(BigInt::from(*value)),
        StackValue::BigInteger(bytes)
        | StackValue::ByteString(bytes)
        | StackValue::Buffer(bytes) => Ok(BigInt::from_signed_bytes_le(bytes)),
        StackValue::Null => Err("Cannot convert Null to Integer".to_string()),
        StackValue::Array(_)
        | StackValue::Struct(_)
        | StackValue::Map(_)
        | StackValue::Interop(_)
        | StackValue::Iterator(_)
        | StackValue::Pointer(_) => Err("Cannot convert to Integer".to_string()),
    }
}

pub fn stack_value_to_bool(value: &StackValue) -> bool {
    value.to_bool()
}

pub fn stack_value_to_string(value: &StackValue) -> Result<String, String> {
    match value {
        StackValue::ByteString(bytes) | StackValue::Buffer(bytes) => {
            String::from_utf8(bytes.clone()).map_err(|err| err.to_string())
        }
        StackValue::Integer(_) | StackValue::BigInteger(_) => {
            Ok(stack_value_to_bigint(value)?.to_string())
        }
        StackValue::Boolean(value) => Ok(value.to_string()),
        _ => Err("Unsupported stack item for string conversion".to_string()),
    }
}

fn integer_stack_value(integer: BigInt) -> StackValue {
    if let Some(value) = integer.to_i64() {
        StackValue::Integer(value)
    } else {
        StackValue::BigInteger(integer.to_signed_bytes_le())
    }
}

fn stack_value_type_name(item: &StackValue) -> &'static str {
    match item {
        StackValue::Null => "Any",
        StackValue::Boolean(_) => "Boolean",
        StackValue::Integer(_) | StackValue::BigInteger(_) => "Integer",
        StackValue::ByteString(_) => "ByteString",
        StackValue::Buffer(_) => "Buffer",
        StackValue::Array(_) => "Array",
        StackValue::Struct(_) => "Struct",
        StackValue::Map(_) => "Map",
        StackValue::Interop(_) | StackValue::Iterator(_) => "InteropInterface",
        StackValue::Pointer(_) => "Pointer",
    }
}
