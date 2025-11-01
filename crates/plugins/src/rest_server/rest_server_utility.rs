// Copyright (C) 2015-2025 The Neo Project.
//
// RestServerUtility ports the helper functions from Neo.Plugins.RestServer.
// At present we provide the script hash/address conversion helpers required by
// the UtilsController; additional helpers will be added as the remaining
// controllers are ported.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use neo_core::neo_system::ProtocolSettings;
use neo_core::wallets::helper::Helper as WalletHelper;
use neo_core::UInt160;
use neo_vm::script::Script;
use neo_vm::stack_item::{StackItem, StackItemType};
use num_bigint::BigInt;
use once_cell::sync::Lazy;
use serde_json::{Map as JsonMap, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RestServerUtilityError {
    #[error("Invalid address format: {0}")]
    InvalidAddress(String),
    #[error("Stack item serialisation error: {0}")]
    StackItem(String),
}

pub struct RestServerUtility;

static EMPTY_SCRIPT: Lazy<Arc<Script>> = Lazy::new(|| Arc::new(Script::new_relaxed(Vec::new())));

impl RestServerUtility {
    /// Converts a textual representation (address or script hash) into a `UInt160`.
    /// Mirrors `ConvertToScriptHash` from the C# utility and propagates parsing errors.
    pub fn convert_to_script_hash(
        address: &str,
        settings: &ProtocolSettings,
    ) -> Result<UInt160, RestServerUtilityError> {
        if let Ok(hash) = address.parse::<UInt160>() {
            return Ok(hash);
        }

        WalletHelper::to_script_hash(address, settings.address_version)
            .map_err(|err| RestServerUtilityError::InvalidAddress(err))
    }

    /// Attempts to convert the supplied value into a script hash, returning `None` when parsing fails.
    /// Mirrors `TryConvertToScriptHash` from the C# utility.
    pub fn try_convert_to_script_hash(
        address: &str,
        settings: &ProtocolSettings,
    ) -> Option<UInt160> {
        match Self::convert_to_script_hash(address, settings) {
            Ok(hash) => Some(hash),
            Err(_) => None,
        }
    }

    /// Serialises a VM [`StackItem`] into the JSON structure used by the C# converter.
    pub fn stack_item_to_j_token(item: &StackItem) -> Result<Value, RestServerUtilityError> {
        let mut context = Vec::new();
        Self::stack_item_to_j_token_internal(item, &mut context)
    }

    /// Deserialises a JSON token into a VM [`StackItem`] (inverse of [`stack_item_to_j_token`]).
    pub fn stack_item_from_j_token(token: &Value) -> Result<StackItem, RestServerUtilityError> {
        Self::stack_item_from_j_token_internal(token)
    }

    fn stack_item_to_j_token_internal(
        item: &StackItem,
        context: &mut Vec<*const StackItem>,
    ) -> Result<Value, RestServerUtilityError> {
        let ptr = item as *const StackItem;
        if context.iter().any(|existing| *existing == ptr) {
            return Ok(serde_json::json!({
                "type": format!("{:?}", StackItemType::Any),
                "value": Value::Null,
            }));
        }

        match item {
            StackItem::Null => Ok(serde_json::json!({
                "type": format!("{:?}", StackItemType::Any),
                "value": Value::Null,
            })),
            StackItem::Boolean(value) => Ok(serde_json::json!({
                "type": format!("{:?}", StackItemType::Boolean),
                "value": value,
            })),
            StackItem::Integer(value) => Ok(serde_json::json!({
                "type": format!("{:?}", StackItemType::Integer),
                "value": value.to_string(),
            })),
            StackItem::ByteString(bytes) => Ok(serde_json::json!({
                "type": format!("{:?}", StackItemType::ByteString),
                "value": BASE64.encode(bytes),
            })),
            StackItem::Buffer(buffer) => Ok(serde_json::json!({
                "type": format!("{:?}", StackItemType::Buffer),
                "value": BASE64.encode(buffer.data()),
            })),
            StackItem::Pointer(pointer) => Ok(serde_json::json!({
                "type": format!("{:?}", StackItemType::Pointer),
                "value": pointer.position(),
            })),
            StackItem::Array(array) => {
                context.push(ptr);
                let mut values = Vec::with_capacity(array.len());
                for entry in array.items() {
                    values.push(Self::stack_item_to_j_token_internal(entry, context)?);
                }
                context.retain(|existing| *existing != ptr);
                Ok(serde_json::json!({
                    "type": format!("{:?}", StackItemType::Array),
                    "value": Value::Array(values),
                }))
            }
            StackItem::Struct(structure) => {
                context.push(ptr);
                let mut values = Vec::with_capacity(structure.len());
                for entry in structure.items() {
                    values.push(Self::stack_item_to_j_token_internal(entry, context)?);
                }
                context.retain(|existing| *existing != ptr);
                Ok(serde_json::json!({
                    "type": format!("{:?}", StackItemType::Struct),
                    "value": Value::Array(values),
                }))
            }
            StackItem::Map(map) => {
                context.push(ptr);
                let mut entries = Vec::with_capacity(map.len());
                for (key, value) in map.items() {
                    let key_json = Self::stack_item_to_j_token_internal(key, context)?;
                    let value_json = Self::stack_item_to_j_token_internal(value, context)?;
                    entries.push(serde_json::json!({
                        "key": key_json,
                        "value": value_json,
                    }));
                }
                context.retain(|existing| *existing != ptr);
                Ok(serde_json::json!({
                    "type": format!("{:?}", StackItemType::Map),
                    "value": Value::Array(entries),
                }))
            }
            StackItem::InteropInterface(_) => Err(RestServerUtilityError::StackItem(
                "InteropInterface stack items are not supported by the REST converter".to_string(),
            )),
        }
    }

    fn stack_item_from_j_token_internal(
        token: &Value,
    ) -> Result<StackItem, RestServerUtilityError> {
        let obj = token.as_object().ok_or_else(|| {
            RestServerUtilityError::StackItem("StackItem JSON must be an object".to_string())
        })?;

        let type_value = Self::get_case_insensitive(obj, "type")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                RestServerUtilityError::StackItem(
                    "StackItem JSON requires a type field".to_string(),
                )
            })?;

        let value_token = Self::get_case_insensitive(obj, "value").ok_or_else(|| {
            RestServerUtilityError::StackItem("StackItem JSON requires a value field".to_string())
        })?;

        let stack_type = Self::parse_stack_item_type(type_value).ok_or_else(|| {
            RestServerUtilityError::StackItem(format!("Unknown StackItemType: {type_value}"))
        })?;

        let item = match stack_type {
            StackItemType::Any => StackItem::null(),
            StackItemType::Boolean => {
                let value = value_token.as_bool().ok_or_else(|| {
                    RestServerUtilityError::StackItem(
                        "Boolean stack item requires bool value".to_string(),
                    )
                })?;
                StackItem::from_bool(value)
            }
            StackItemType::Integer => {
                let text = match value_token {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    _ => {
                        return Err(RestServerUtilityError::StackItem(
                            "Integer stack item requires string or number value".to_string(),
                        ))
                    }
                };
                let bigint = BigInt::parse_bytes(text.as_bytes(), 10).ok_or_else(|| {
                    RestServerUtilityError::StackItem("Invalid integer value".to_string())
                })?;
                StackItem::from_int(bigint)
            }
            StackItemType::ByteString => {
                let text = value_token.as_str().ok_or_else(|| {
                    RestServerUtilityError::StackItem(
                        "ByteString stack item requires base64 string".to_string(),
                    )
                })?;
                let bytes = BASE64.decode(text.as_bytes()).map_err(|err| {
                    RestServerUtilityError::StackItem(format!("Invalid base64: {err}"))
                })?;
                StackItem::from_byte_string(bytes)
            }
            StackItemType::Buffer => {
                let text = value_token.as_str().ok_or_else(|| {
                    RestServerUtilityError::StackItem(
                        "Buffer stack item requires base64 string".to_string(),
                    )
                })?;
                let bytes = BASE64.decode(text.as_bytes()).map_err(|err| {
                    RestServerUtilityError::StackItem(format!("Invalid base64: {err}"))
                })?;
                StackItem::from_buffer(bytes)
            }
            StackItemType::Array => {
                let array = value_token.as_array().ok_or_else(|| {
                    RestServerUtilityError::StackItem(
                        "Array stack item requires array value".to_string(),
                    )
                })?;
                let mut items = Vec::with_capacity(array.len());
                for entry in array {
                    items.push(Self::stack_item_from_j_token_internal(entry)?);
                }
                StackItem::from_array(items)
            }
            StackItemType::Struct => {
                let array = value_token.as_array().ok_or_else(|| {
                    RestServerUtilityError::StackItem(
                        "Struct stack item requires array value".to_string(),
                    )
                })?;
                let mut items = Vec::with_capacity(array.len());
                for entry in array {
                    items.push(Self::stack_item_from_j_token_internal(entry)?);
                }
                StackItem::from_struct(items)
            }
            StackItemType::Map => {
                let array = value_token.as_array().ok_or_else(|| {
                    RestServerUtilityError::StackItem(
                        "Map stack item requires array value".to_string(),
                    )
                })?;
                let mut entries = BTreeMap::new();
                for entry in array {
                    let obj = entry.as_object().ok_or_else(|| {
                        RestServerUtilityError::StackItem(
                            "Map entry must be an object with key/value".to_string(),
                        )
                    })?;

                    let key_token = Self::get_case_insensitive(obj, "key").ok_or_else(|| {
                        RestServerUtilityError::StackItem("Map entry missing key".to_string())
                    })?;
                    let value_token =
                        Self::get_case_insensitive(obj, "value").ok_or_else(|| {
                            RestServerUtilityError::StackItem("Map entry missing value".to_string())
                        })?;

                    let key_item = Self::stack_item_from_j_token_internal(key_token)?;
                    let value_item = Self::stack_item_from_j_token_internal(value_token)?;
                    entries.insert(key_item, value_item);
                }
                StackItem::from_map(entries)
            }
            StackItemType::Pointer => {
                let position = value_token.as_i64().ok_or_else(|| {
                    RestServerUtilityError::StackItem("Pointer value must be integer".to_string())
                })?;
                let position = usize::try_from(position).map_err(|_| {
                    RestServerUtilityError::StackItem("Pointer position out of range".to_string())
                })?;
                StackItem::from_pointer(Arc::clone(&EMPTY_SCRIPT), position)
            }
            StackItemType::InteropInterface => {
                return Err(RestServerUtilityError::StackItem(
                    "InteropInterface deserialisation is not supported".to_string(),
                ));
            }
        };

        Ok(item)
    }

    fn parse_stack_item_type(name: &str) -> Option<StackItemType> {
        match name.to_lowercase().as_str() {
            "any" => Some(StackItemType::Any),
            "boolean" => Some(StackItemType::Boolean),
            "integer" => Some(StackItemType::Integer),
            "bytestring" => Some(StackItemType::ByteString),
            "buffer" => Some(StackItemType::Buffer),
            "array" => Some(StackItemType::Array),
            "struct" => Some(StackItemType::Struct),
            "map" => Some(StackItemType::Map),
            "pointer" => Some(StackItemType::Pointer),
            "interopinterface" => Some(StackItemType::InteropInterface),
            _ => None,
        }
    }

    fn get_case_insensitive<'a>(map: &'a JsonMap<String, Value>, name: &str) -> Option<&'a Value> {
        let lowered = name.to_lowercase();
        map.iter()
            .find(|(key, _)| key.to_lowercase() == lowered)
            .map(|(_, value)| value)
    }
}
