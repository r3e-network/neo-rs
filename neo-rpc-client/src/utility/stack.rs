use std::any::Any;
use std::collections::BTreeMap;
use std::sync::Arc;

use neo_json::JObject;
use neo_vm::stack_item::InteropInterface;
use neo_vm::{Script, StackItem};
use num_bigint::BigInt;
use serde_json::Value as JsonValue;

use crate::utility::parsing::{jobject_to_serde, parse_base64_token, parse_u32_token};

/// Converts a `neo-json` representation of a stack item back into a VM stack item.
pub fn stack_item_from_json(json: &JObject) -> Result<StackItem, String> {
    let item_type = json
        .get("type")
        .and_then(|v| v.as_string())
        .ok_or("StackItem entry missing 'type' field")?;

    match item_type.as_str() {
        "Any" => Ok(StackItem::null()),
        "Boolean" => {
            let value = json
                .get("value")
                .map(|token| token.as_boolean())
                .ok_or("Boolean stack item missing 'value' field")?;
            Ok(StackItem::from_bool(value))
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
            Ok(StackItem::from_int(integer))
        }
        "ByteString" => {
            let value_token = json
                .get("value")
                .ok_or("ByteString stack item missing 'value' field")?;
            let data = parse_base64_token(value_token, "value")?;
            Ok(StackItem::from_byte_string(data))
        }
        "Buffer" => {
            let value_token = json
                .get("value")
                .ok_or("Buffer stack item missing 'value' field")?;
            let data = parse_base64_token(value_token, "value")?;
            Ok(StackItem::from_buffer(data))
        }
        "Array" => {
            let values = json
                .get("value")
                .and_then(|token| token.as_array())
                .ok_or("Array stack item missing 'value' array")?;
            let mut items = Vec::with_capacity(values.len());
            for value in values.children() {
                let token = value.as_ref().ok_or("Array entries must be objects")?;
                let obj = token.as_object().ok_or("Array entries must be objects")?;
                items.push(stack_item_from_json(obj)?);
            }
            Ok(StackItem::from_array(items))
        }
        "Struct" => {
            let values = json
                .get("value")
                .and_then(|token| token.as_array())
                .ok_or("Struct stack item missing 'value' array")?;
            let mut items = Vec::with_capacity(values.len());
            for value in values.children() {
                let token = value.as_ref().ok_or("Struct entries must be objects")?;
                let obj = token.as_object().ok_or("Struct entries must be objects")?;
                items.push(stack_item_from_json(obj)?);
            }
            Ok(StackItem::from_struct(items))
        }
        "Map" => {
            let entries = json
                .get("value")
                .and_then(|token| token.as_array())
                .ok_or("Map stack item missing 'value' array")?;
            #[allow(clippy::mutable_key_type)]
            let mut map = BTreeMap::new();
            for entry in entries.children() {
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
                let key = stack_item_from_json(key_obj)?;
                let value = stack_item_from_json(value_obj)?;
                map.insert(key, value);
            }
            Ok(StackItem::from_map(map))
        }
        "Pointer" => {
            let index_token = json
                .get("value")
                .ok_or("Pointer stack item missing 'value' field")?;
            let index = parse_u32_token(index_token, "value")? as usize;
            let script = Arc::new(Script::new_relaxed(Vec::new()));
            Ok(StackItem::from_pointer(script, index))
        }
        "InteropInterface" => {
            let payload = json
                .get("value")
                .and_then(|token| token.as_object())
                .ok_or("InteropInterface missing 'value' object")?;
            let serde_payload = jobject_to_serde(payload)?;
            Ok(StackItem::from_interface(JsonInteropInterface::new(
                serde_payload,
            )))
        }
        other => Err(format!(
            "Unsupported stack item type '{other}' in JSON payload"
        )),
    }
}

#[derive(Debug)]
struct JsonInteropInterface {
    _payload: JsonValue,
}

impl JsonInteropInterface {
    fn new(payload: JsonValue) -> Self {
        Self { _payload: payload }
    }
}

impl InteropInterface for JsonInteropInterface {
    fn interface_type(&self) -> &str {
        "json"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
