use std::collections::VecDeque;
use std::io::Write;
use std::convert::TryFrom;
use serde_json::{ json};
use neo_json::jtoken::{JToken, MAX_SAFE_INTEGER, MIN_SAFE_INTEGER};
use neo_vm::execution_engine_limits::ExecutionEngineLimits;
use neo_vm::reference_counter::ReferenceCounter;
use neo_vm::stack_item::StackItem;
use neo_vm::vm::ExecutionEngineLimits;
use neo_vm::vm_types::reference_counter::ReferenceCounter;
use neo_vm::vm_types::stack_item::StackItem;
use crate::hardfork::Hardfork;
use crate::neo_contract::application_engine::ApplicationEngine;

/// A JSON serializer for `StackItem`.
pub struct JsonSerializer;

impl JsonSerializer {
    /// Serializes a `StackItem` to a `JToken`.
    pub fn serialize(item: &StackItem) -> Result<JToken, String> {
        match item {
            StackItem::Array(array) => {
                Ok(JToken::Array(array.iter().map(|p| Self::serialize(p)).collect::<Result<Vec<_>, _>>()?))
            },
            StackItem::ByteString(_) | StackItem::Buffer(_) => {
                Ok(JToken::String(item.get_string()?))
            },
            StackItem::Integer(num) => {
                let integer = num.get_integer()?;
                if integer > MAX_SAFE_INTEGER || integer < MIN_SAFE_INTEGER {
                    return Err("Integer out of safe range".into());
                }
                Ok(JToken::Number(integer as f64))
            },
            StackItem::Boolean(boolean) => {
                Ok(JToken::Boolean(boolean.get_boolean()?))
            },
            StackItem::Map(map) => {
                let mut ret = JToken::new_object();
                for (key, value) in map.iter() {
                    if !matches!(key, StackItem::ByteString(_)) {
                        return Err("Map key must be ByteString".into());
                    }
                    let key_str = key.get_string()?;
                    let value_token = Self::serialize(value)?;
                    ret.insert(key_str, value_token).expect("TODO: panic message");
                }
                Ok(JToken::Object(ret))
            },
            StackItem::Null => {
                Ok(JToken::Null)
            },
            _ => Err("Unsupported StackItem type".into()),
        }
    }

    /// Serializes a `StackItem` to JSON.
    pub fn serialize_to_byte_array(item: &StackItem, max_size: u32) -> Result<Vec<u8>, String> {
        let mut output = Vec::new();
        let mut stack = VecDeque::new();
        stack.push_back(item);

        while let Some(current) = stack.pop_back() {
            match current {
                StackItem::Array(array) => {
                    output.write_all(b"[").map_err(|e| e.to_string())?;
                    stack.push_back(&JsonTokenType::EndArray);
                    for item in array.iter().rev() {
                        stack.push_back(item);
                    }
                },
                StackItem::ByteString(_) | StackItem::Buffer(_) => {
                    let value = json!(current.get_string()?);
                    serde_json::to_writer(&mut output, &value).map_err(|e| e.to_string())?;
                },
                StackItem::Integer(num) => {
                    let integer = num.get_integer()?;
                    if integer > MAX_SAFE_INTEGER || integer < MIN_SAFE_INTEGER {
                        return Err("Integer out of safe range".into());
                    }
                    let value = json!(integer);
                    serde_json::to_writer(&mut output, &value).map_err(|e| e.to_string())?;
                },
                StackItem::Boolean(boolean) => {
                    let value = json!(boolean.get_boolean()?);
                    serde_json::to_writer(&mut output, &value).map_err(|e| e.to_string())?;
                },
                StackItem::Map(map) => {
                    output.write_all(b"{").map_err(|e| e.to_string())?;
                    stack.push_back(&JsonTokenType::EndObject);
                    for (key, value) in map.iter().rev() {
                        if !matches!(key, StackItem::ByteString(_)) {
                            return Err("Map key must be ByteString".into());
                        }
                        stack.push_back(value);
                        stack.push_back(key);
                        stack.push_back(&JsonTokenType::PropertyName);
                    }
                },
                StackItem::Null => {
                    output.write_all(b"null").map_err(|e| e.to_string())?;
                },
                JsonTokenType::EndArray => {
                    output.write_all(b"]").map_err(|e| e.to_string())?;
                },
                JsonTokenType::EndObject => {
                    output.write_all(b"}").map_err(|e| e.to_string())?;
                },
                JsonTokenType::PropertyName => {
                    if let Some(key) = stack.pop_back() {
                        let key_str = key.get_string()?;
                        let value = json!(key_str);
                        serde_json::to_writer(&mut output, &value).map_err(|e| e.to_string())?;
                        output.write_all(b":").map_err(|e| e.to_string())?;
                    }
                },
            }

            if output.len() as u32 > max_size {
                return Err("Serialized output exceeds maximum size".into());
            }
        }

        Ok(output)
    }

    /// Deserializes a `StackItem` from `JToken`.
    pub fn deserialize(engine: &ApplicationEngine, json: &JToken, limits: &ExecutionEngineLimits, reference_counter: Option<&ReferenceCounter>) -> Result<StackItem, String> {
        let mut max_stack_size = limits.max_stack_size;
        Self::deserialize_internal(engine, json, &mut max_stack_size, reference_counter)
    }

    fn deserialize_internal(engine: &ApplicationEngine, json: &JToken, max_stack_size: &mut usize, reference_counter: Option<&ReferenceCounter>) -> Result<StackItem, String> {
        if *max_stack_size == 0 {
            return Err("Max stack size exceeded".into());
        }
        *max_stack_size -= 1;

        match json {
            JToken::Null => Ok(StackItem::Null),
            JToken::Array(array) => {
                let mut list = Vec::with_capacity(array.len());
                for obj in array {
                    list.push(Self::deserialize_internal(engine, obj, max_stack_size, reference_counter)?);
                }
                Ok(StackItem::new_array(reference_counter, list))
            },
            JToken::String(str) => Ok(StackItem::ByteString(str.as_bytes().to_vec())),
            JToken::Number(num) => {
                if num.fract() != 0.0 {
                    return Err("Decimal value is not allowed".into());
                }
                if engine.is_hardfork_enabled(Hardfork::HF_Basilisk) {
                    Ok(StackItem::Integer(num.to_string().parse::<i64>().map_err(|e| e.to_string())?))
                } else {
                    Ok(StackItem::Integer(Integer::try_from(num.to_string().parse::<i64>().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?))
                }
            },
            JToken::Boolean(boolean) => Ok(StackItem::Boolean(*boolean)),
            JToken::Object(obj) => {
                let mut map = Map::new(reference_counter);
                for (key, value) in obj {
                    if *max_stack_size == 0 {
                        return Err("Max stack size exceeded".into());
                    }
                    *max_stack_size -= 1;

                    let key_item = StackItem::ByteString(key.as_bytes());
                    let value_item = Self::deserialize_internal(engine, value, max_stack_size, reference_counter)?;
                    map.insert(key_item, value_item);
                }
                Ok(StackItem::Map(map))
            },
        }
    }
}

enum JsonTokenType {
    EndArray,
    EndObject,
    PropertyName,
}
