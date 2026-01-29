use std::any::Any;
use std::sync::Arc;

use base64::{engine::general_purpose, Engine as _};
use neo_json::{JArray, JObject, JToken};
use neo_vm::stack_item::InteropInterface;
use neo_vm::{OrderedDictionary, Script, StackItem};
use num_bigint::BigInt;
use serde_json::Value as JsonValue;
use std::collections::HashSet;

use super::parsing::{jobject_to_serde, parse_base64_token, parse_u32_token};

/// Converts a `neo-json` representation of a stack item back into a VM stack item.
pub fn stack_item_from_json(json: &JObject) -> Result<StackItem, String> {
    let item_type = json
        .get("type")
        .and_then(neo_json::JToken::as_string)
        .ok_or("StackItem entry missing 'type' field")?;

    match item_type.as_str() {
        "Any" => {
            let value = json.get("value");
            let text = value
                .and_then(|token| token.as_string())
                .or_else(|| value.map(std::string::ToString::to_string));
            if let Some(text) = text {
                Ok(StackItem::from_byte_string(text.into_bytes()))
            } else {
                Ok(StackItem::null())
            }
        }
        "Boolean" => {
            let value = json
                .get("value")
                .map(neo_json::JToken::as_boolean)
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
            let mut map = OrderedDictionary::new();
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
            let serde_payload = jobject_to_serde(json)?;
            Ok(StackItem::from_interface(JsonInteropInterface::new(
                serde_payload,
            )))
        }
        _other => {
            let value = json.get("value");
            let text = value
                .and_then(|token| token.as_string())
                .or_else(|| value.map(std::string::ToString::to_string));

            if let Some(text) = text {
                Ok(StackItem::from_byte_string(text.into_bytes()))
            } else {
                Ok(StackItem::null())
            }
        }
    }
}

/// Converts a VM stack item into its `neo-json` representation.
pub fn stack_item_to_json(item: &StackItem) -> Result<JObject, String> {
    let mut context = HashSet::new();
    stack_item_to_json_inner(item, &mut context)
}

fn stack_item_to_json_inner(
    item: &StackItem,
    context: &mut HashSet<usize>,
) -> Result<JObject, String> {
    let mut json = JObject::new();
    json.insert(
        "type".to_string(),
        JToken::String(format!("{:?}", item.stack_item_type())),
    );

    match item {
        StackItem::Null | StackItem::InteropInterface(_) => {}
        StackItem::Boolean(value) => {
            json.insert("value".to_string(), JToken::Boolean(*value));
        }
        StackItem::Integer(value) => {
            json.insert("value".to_string(), JToken::String(value.to_string()));
        }
        StackItem::ByteString(bytes) => {
            json.insert(
                "value".to_string(),
                JToken::String(general_purpose::STANDARD.encode(bytes)),
            );
        }
        StackItem::Buffer(buffer) => {
            json.insert(
                "value".to_string(),
                JToken::String(general_purpose::STANDARD.encode(buffer.data())),
            );
        }
        StackItem::Pointer(pointer) => {
            json.insert(
                "value".to_string(),
                JToken::Number(pointer.position() as f64),
            );
        }
        StackItem::Array(array) => {
            let id = array.id();
            if !context.insert(id) {
                return Err("Circular reference.".to_string());
            }
            let entries = array
                .items()
                .iter()
                .map(|entry| stack_item_to_json_inner(entry, context))
                .collect::<Result<Vec<_>, _>>();
            context.remove(&id);
            let entries = entries?;
            let values = entries.into_iter().map(JToken::Object).collect::<Vec<_>>();
            json.insert("value".to_string(), JToken::Array(JArray::from(values)));
        }
        StackItem::Struct(structure) => {
            let id = structure.id();
            if !context.insert(id) {
                return Err("Circular reference.".to_string());
            }
            let entries = structure
                .items()
                .iter()
                .map(|entry| stack_item_to_json_inner(entry, context))
                .collect::<Result<Vec<_>, _>>();
            context.remove(&id);
            let entries = entries?;
            let values = entries.into_iter().map(JToken::Object).collect::<Vec<_>>();
            json.insert("value".to_string(), JToken::Array(JArray::from(values)));
        }
        StackItem::Map(map) => {
            let id = map.id();
            if !context.insert(id) {
                return Err("Circular reference.".to_string());
            }
            let entries = map
                .items()
                .iter()
                .map(|(key, value)| {
                    let key_json = stack_item_to_json_inner(key, context)?;
                    let value_json = stack_item_to_json_inner(value, context)?;
                    let mut entry = JObject::new();
                    entry.insert("key".to_string(), JToken::Object(key_json));
                    entry.insert("value".to_string(), JToken::Object(value_json));
                    Ok(JToken::Object(entry))
                })
                .collect::<Result<Vec<_>, String>>();
            context.remove(&id);
            let entries = entries?;
            json.insert("value".to_string(), JToken::Array(JArray::from(entries)));
        }
    }

    Ok(json)
}

#[derive(Debug)]
struct JsonInteropInterface {
    _payload: JsonValue,
}

impl JsonInteropInterface {
    const fn new(payload: JsonValue) -> Self {
        Self { _payload: payload }
    }
}

impl InteropInterface for JsonInteropInterface {
    fn interface_type(&self) -> &'static str {
        "json"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
