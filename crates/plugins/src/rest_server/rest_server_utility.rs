//! Partial port of `RestServerUtility.cs`.
//! Provides helpers for converting addresses and handling VM stack items.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use neo_core::{error::CoreError, neo_system::ProtocolSettings, UInt160};
use neo_extensions::{ExtensionError, ExtensionResult};
use neo_vm::{
    stack_item::stack_item::InteropInterface as VmInteropInterface, StackItem, StackItemType,
};
use num_bigint::BigInt;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;
use std::sync::Arc;

/// Convert a string (address or script hash) into a `UInt160`.
/// Mirrors the behaviour of the C# `RestServerUtility.ConvertToScriptHash` method.
pub fn convert_to_script_hash(
    address: &str,
    settings: &ProtocolSettings,
) -> ExtensionResult<UInt160> {
    if let Ok(hash) = UInt160::from_str(address) {
        return Ok(hash);
    }

    address_to_script_hash(address, settings.address_version)
}

/// Try-convert helper returning `Option` instead of propagating errors.
pub fn try_convert_to_script_hash(address: &str, settings: &ProtocolSettings) -> Option<UInt160> {
    convert_to_script_hash(address, settings).ok()
}

fn address_to_script_hash(address: &str, version: u8) -> ExtensionResult<UInt160> {
    let decoded = bs58::decode(address)
        .into_vec()
        .map_err(|_| ExtensionError::invalid_config("invalid Base58 address"))?;

    if decoded.len() != 25 {
        return Err(ExtensionError::invalid_config("invalid address length"));
    }

    if decoded[0] != version {
        return Err(ExtensionError::invalid_config("address version mismatch"));
    }

    let data = &decoded[..21];
    let checksum = &decoded[21..];

    let mut hasher = Sha256::new();
    hasher.update(data);
    let first_hash = hasher.finalize();

    let mut hasher = Sha256::new();
    hasher.update(first_hash);
    let second_hash = hasher.finalize();

    if checksum != &second_hash[..4] {
        return Err(ExtensionError::invalid_config("invalid address checksum"));
    }

    UInt160::from_bytes(&decoded[1..21]).map_err(map_core_error)
}

fn map_core_error(err: CoreError) -> ExtensionError {
    match err {
        CoreError::InvalidFormat { message } => ExtensionError::invalid_config(message),
        other => ExtensionError::invalid_config(other.to_string()),
    }
}

/// Parse a stack item from a JSON value (mirrors `StackItemFromJToken`).
pub fn stack_item_from_json(value: &Value) -> ExtensionResult<StackItem> {
    match value {
        Value::Null => Ok(StackItem::Null),
        Value::Object(map) => stack_item_from_object(map),
        _ => Err(ExtensionError::invalid_config("invalid stack item json")),
    }
}

fn stack_item_from_object(map: &Map<String, Value>) -> ExtensionResult<StackItem> {
    let type_token = map
        .get("type")
        .or_else(|| map.get("Type"))
        .and_then(Value::as_str)
        .ok_or_else(|| ExtensionError::invalid_config("stack item missing type"))?;
    let stack_item_type = parse_stack_item_type(type_token)
        .ok_or_else(|| ExtensionError::invalid_config("unsupported stack item type"))?;
    let value_token = map
        .get("value")
        .or_else(|| map.get("Value"))
        .unwrap_or(&Value::Null);

    match stack_item_type {
        StackItemType::Struct => {
            let array = value_token
                .as_array()
                .ok_or_else(|| ExtensionError::invalid_config("struct requires array value"))?;
            let mut items = Vec::with_capacity(array.len());
            for item in array {
                items.push(stack_item_from_json(item)?);
            }
            Ok(StackItem::from_struct(items))
        }
        StackItemType::Array => {
            let array = value_token
                .as_array()
                .ok_or_else(|| ExtensionError::invalid_config("array requires array value"))?;
            let mut items = Vec::with_capacity(array.len());
            for item in array {
                items.push(stack_item_from_json(item)?);
            }
            Ok(StackItem::from_array(items))
        }
        StackItemType::Map => {
            let entries = value_token
                .as_array()
                .ok_or_else(|| ExtensionError::invalid_config("map requires array value"))?;
            let mut map_items = BTreeMap::new();
            for entry in entries {
                let obj = entry
                    .as_object()
                    .ok_or_else(|| ExtensionError::invalid_config("map entry must be object"))?;
                let key_token = obj
                    .get("key")
                    .or_else(|| obj.get("Key"))
                    .ok_or_else(|| ExtensionError::invalid_config("map entry missing key"))?;
                let value_token = obj
                    .get("value")
                    .or_else(|| obj.get("Value"))
                    .ok_or_else(|| ExtensionError::invalid_config("map entry missing value"))?;
                let key = stack_item_from_json(key_token)?;
                let value = stack_item_from_json(value_token)?;
                map_items.insert(key, value);
            }
            Ok(StackItem::from_map(map_items))
        }
        StackItemType::Boolean => {
            let flag = match value_token {
                Value::Bool(b) => *b,
                Value::String(s) => s
                    .parse::<bool>()
                    .map_err(|_| ExtensionError::invalid_config("invalid boolean"))?,
                _ => return Err(ExtensionError::invalid_config("boolean value expected")),
            };
            Ok(StackItem::Boolean(flag))
        }
        StackItemType::Buffer => {
            let bytes = parse_base64(value_token)?;
            Ok(StackItem::from_buffer(bytes))
        }
        StackItemType::ByteString => {
            let bytes = parse_base64(value_token)?;
            Ok(StackItem::from_byte_string(bytes))
        }
        StackItemType::Integer => {
            let big = parse_big_int(value_token)?;
            Ok(StackItem::Integer(big))
        }
        StackItemType::InteropInterface => {
            let bytes = parse_base64(value_token)?;
            Ok(StackItem::InteropInterface(Arc::new(
                RawInteropInterface::new(bytes),
            )))
        }
        StackItemType::Pointer => {
            let offset = parse_usize(value_token)?;
            Ok(StackItem::Pointer(offset))
        }
        StackItemType::Any => Ok(StackItem::Null),
    }
}

fn parse_stack_item_type(value: &str) -> Option<StackItemType> {
    match value.to_lowercase().as_str() {
        "struct" => Some(StackItemType::Struct),
        "array" => Some(StackItemType::Array),
        "map" => Some(StackItemType::Map),
        "boolean" => Some(StackItemType::Boolean),
        "buffer" => Some(StackItemType::Buffer),
        "bytestring" => Some(StackItemType::ByteString),
        "integer" => Some(StackItemType::Integer),
        "interopinterface" => Some(StackItemType::InteropInterface),
        "pointer" => Some(StackItemType::Pointer),
        "any" | "null" => Some(StackItemType::Any),
        _ => None,
    }
}

fn parse_base64(value: &Value) -> ExtensionResult<Vec<u8>> {
    let encoded = value
        .as_str()
        .ok_or_else(|| ExtensionError::invalid_config("expected base64 string"))?;
    STANDARD
        .decode(encoded)
        .map_err(|_| ExtensionError::invalid_config("invalid base64 data"))
}

fn parse_big_int(value: &Value) -> ExtensionResult<BigInt> {
    match value {
        Value::Number(num) => {
            if let Some(n) = num.as_i64() {
                Ok(BigInt::from(n))
            } else if let Some(n) = num.as_u64() {
                Ok(BigInt::from(n))
            } else {
                Err(ExtensionError::invalid_config("unsupported number"))
            }
        }
        Value::String(s) => {
            BigInt::from_str(s).map_err(|_| ExtensionError::invalid_config("invalid integer value"))
        }
        _ => Err(ExtensionError::invalid_config("integer value expected")),
    }
}

fn parse_usize(value: &Value) -> ExtensionResult<usize> {
    match value {
        Value::Number(num) => num
            .as_i64()
            .and_then(|n| if n >= 0 { Some(n as usize) } else { None })
            .ok_or_else(|| ExtensionError::invalid_config("invalid pointer value")),
        Value::String(s) => {
            let parsed = s
                .parse::<i64>()
                .map_err(|_| ExtensionError::invalid_config("invalid pointer value"))?;
            if parsed < 0 {
                Err(ExtensionError::invalid_config("invalid pointer value"))
            } else {
                Ok(parsed as usize)
            }
        }
        _ => Err(ExtensionError::invalid_config("invalid pointer value")),
    }
}

/// Convert a stack item into a JSON value (mirrors `StackItemToJToken`).
pub fn stack_item_to_json(item: &StackItem) -> Value {
    let mut cache = HashMap::new();
    stack_item_to_json_internal(item, &mut cache)
}

fn stack_item_to_json_internal<'a>(
    item: &'a StackItem,
    cache: &mut HashMap<usize, Value>,
) -> Value {
    let key = item as *const StackItem as usize;
    if let Some(existing) = cache.get(&key) {
        return existing.clone();
    }

    match item {
        StackItem::Null => Value::Null,
        StackItem::Boolean(value) => json!({"Type": "Boolean", "Value": value}),
        StackItem::Integer(value) => json!({"Type": "Integer", "Value": value.to_string()}),
        StackItem::ByteString(bytes) => {
            json!({"Type": "ByteString", "Value": STANDARD.encode(bytes)})
        }
        StackItem::Buffer(bytes) => json!({"Type": "Buffer", "Value": STANDARD.encode(bytes)}),
        StackItem::Pointer(offset) => json!({"Type": "Pointer", "Value": *offset as i64}),
        StackItem::InteropInterface(interface) => {
            let bytes = interface
                .as_any()
                .downcast_ref::<RawInteropInterface>()
                .map(|raw| raw.data.clone())
                .unwrap_or_else(|| interface.interface_type().as_bytes().to_vec());
            json!({"Type": "InteropInterface", "Value": STANDARD.encode(&bytes)})
        }
        StackItem::Array(items) => {
            let placeholder = json!({"Type": "Array", "Value": []});
            cache.insert(key, placeholder.clone());
            let values: Vec<Value> = items
                .iter()
                .map(|child| stack_item_to_json_internal(child, cache))
                .collect();
            let result = json!({"Type": "Array", "Value": values});
            cache.insert(key, result.clone());
            result
        }
        StackItem::Struct(items) => {
            let placeholder = json!({"Type": "Struct", "Value": []});
            cache.insert(key, placeholder.clone());
            let values: Vec<Value> = items
                .iter()
                .map(|child| stack_item_to_json_internal(child, cache))
                .collect();
            let result = json!({"Type": "Struct", "Value": values});
            cache.insert(key, result.clone());
            result
        }
        StackItem::Map(map_items) => {
            let placeholder = json!({"Type": "Map", "Value": []});
            cache.insert(key, placeholder.clone());
            let values: Vec<Value> = map_items
                .iter()
                .map(|(key_item, value_item)| {
                    json!({
                        "Key": stack_item_to_json_internal(key_item, cache),
                        "Value": stack_item_to_json_internal(value_item, cache)
                    })
                })
                .collect();
            let result = json!({"Type": "Map", "Value": values});
            cache.insert(key, result.clone());
            result
        }
    }
}

#[derive(Debug)]
struct RawInteropInterface {
    data: Vec<u8>,
}

impl RawInteropInterface {
    fn new(data: Vec<u8>) -> Self {
        Self { data }
    }
}

impl VmInteropInterface for RawInteropInterface {
    fn interface_type(&self) -> &str {
        "RawInterop"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_core::neo_system::ProtocolSettings;

    #[test]
    fn convert_to_script_hash_roundtrip() {
        let hash = UInt160::from_str("0x0123456789abcdef0123456789abcdef01234567").unwrap();
        let address = hash.to_address();
        let settings = ProtocolSettings::new();

        let converted = convert_to_script_hash(&address, &settings).unwrap();
        assert_eq!(converted, hash);

        // Direct script-hash string should also parse
        let from_hash = convert_to_script_hash(&hash.to_string(), &settings).unwrap();
        assert_eq!(from_hash, hash);
    }

    #[test]
    fn stack_item_roundtrip() {
        let item = StackItem::from_array(vec![
            StackItem::Boolean(true),
            StackItem::Integer(BigInt::from(42)),
            StackItem::ByteString(vec![1, 2, 3]),
        ]);

        let json = stack_item_to_json(&item);
        let parsed = stack_item_from_json(&json).unwrap();
        assert_eq!(parsed, item);
    }
}
