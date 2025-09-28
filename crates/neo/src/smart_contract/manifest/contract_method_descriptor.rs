//! ContractMethodDescriptor - matches C# Neo.SmartContract.Manifest.ContractMethodDescriptor exactly

use crate::smart_contract::i_interoperable::IInteroperable;
use crate::smart_contract::manifest::{ContractEventDescriptor, ContractParameterDefinition};
use crate::smart_contract::ContractParameterType;
use neo_vm::StackItem;
use num_traits::ToPrimitive;
use serde::{Deserialize, Serialize};

/// Represents a method in a smart contract ABI (matches C# ContractMethodDescriptor)
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractMethodDescriptor {
    /// The name of the method
    pub name: String,

    /// The parameters of the method
    pub parameters: Vec<ContractParameterDefinition>,

    /// The return type of the method
    pub return_type: ContractParameterType,

    /// The position of the method in the contract script
    pub offset: i32,

    /// Indicates whether the method is a safe method
    pub safe: bool,
}

impl ContractMethodDescriptor {
    /// Creates a new method descriptor
    pub fn new(
        name: String,
        parameters: Vec<ContractParameterDefinition>,
        return_type: ContractParameterType,
        offset: i32,
        safe: bool,
    ) -> Result<Self, String> {
        if name.is_empty() {
            return Err("Method name cannot be empty".to_string());
        }

        if offset < 0 {
            return Err("Offset cannot be negative".to_string());
        }

        // Check for duplicate parameter names
        let mut names = std::collections::HashSet::new();
        for param in &parameters {
            if !names.insert(&param.name) {
                return Err(format!("Duplicate parameter name: {}", param.name));
            }
        }

        Ok(Self {
            name,
            parameters,
            return_type,
            offset,
            safe,
        })
    }

    /// Creates from JSON
    pub fn from_json(json: &serde_json::Value) -> Result<Self, String> {
        let obj = json.as_object().ok_or("Expected object")?;

        let name = obj
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or("Missing name")?
            .to_string();

        let parameters = obj
            .get("parameters")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|p| ContractParameterDefinition::from_json(p).ok())
                    .collect()
            })
            .unwrap_or_default();

        let return_type = obj
            .get("returntype")
            .and_then(|v| v.as_str())
            .and_then(|s| ContractParameterType::from_string(s).ok())
            .unwrap_or(ContractParameterType::Void);

        let offset = obj.get("offset").and_then(|v| v.as_i64()).unwrap_or(0) as i32;

        let safe = obj.get("safe").and_then(|v| v.as_bool()).unwrap_or(false);

        Self::new(name, parameters, return_type, offset, safe)
    }

    /// Converts to JSON
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "parameters": self.parameters.iter().map(|p| p.to_json()).collect::<Vec<_>>(),
            "returntype": format!("{:?}", self.return_type),
            "offset": self.offset,
            "safe": self.safe,
        })
    }
}

impl IInteroperable for ContractMethodDescriptor {
    fn from_stack_item(&mut self, stack_item: StackItem) {
        if let StackItem::Struct(struct_item) = stack_item {
            let items = struct_item.items();
            if items.len() < 5 {
                return;
            }

            if let Ok(bytes) = items[0].as_bytes() {
                if let Ok(name) = String::from_utf8(bytes) {
                    self.name = name;
                }
            }

            if let Ok(param_items) = items[1].as_array() {
                self.parameters = param_items
                    .iter()
                    .map(|item| {
                        let mut param = ContractParameterDefinition::default();
                        param.from_stack_item(item.clone());
                        param
                    })
                    .collect();
            }

            if let Ok(integer) = items[2].as_int() {
                if let Some(byte_val) = integer.to_u8() {
                    self.return_type = ContractParameterType::from_byte(byte_val)
                        .unwrap_or(ContractParameterType::Void);
                }
            }

            if let Ok(integer) = items[3].as_int() {
                if let Some(offset) = integer.to_i32() {
                    self.offset = offset;
                }
            }

            if let Ok(flag) = items[4].as_bool() {
                self.safe = flag;
            }
        }
    }

    fn to_stack_item(&self) -> StackItem {
        let params = self
            .parameters
            .iter()
            .map(|p| p.to_stack_item())
            .collect::<Vec<_>>();
        StackItem::from_struct(vec![
            StackItem::from_byte_string(self.name.as_bytes()),
            StackItem::from_array(params),
            StackItem::from_int(self.return_type as u8),
            StackItem::from_int(self.offset),
            StackItem::from_bool(self.safe),
        ])
    }

    fn clone_box(&self) -> Box<dyn IInteroperable> {
        Box::new(self.clone())
    }
}

// Helper extension for ContractParameterType
impl ContractParameterType {
    pub fn from_string(s: &str) -> Result<Self, String> {
        match s {
            "Any" => Ok(ContractParameterType::Any),
            "Boolean" => Ok(ContractParameterType::Boolean),
            "Integer" => Ok(ContractParameterType::Integer),
            "ByteArray" => Ok(ContractParameterType::ByteArray),
            "String" => Ok(ContractParameterType::String),
            "Hash160" => Ok(ContractParameterType::Hash160),
            "Hash256" => Ok(ContractParameterType::Hash256),
            "PublicKey" => Ok(ContractParameterType::PublicKey),
            "Signature" => Ok(ContractParameterType::Signature),
            "Array" => Ok(ContractParameterType::Array),
            "Map" => Ok(ContractParameterType::Map),
            "InteropInterface" => Ok(ContractParameterType::InteropInterface),
            "Void" => Ok(ContractParameterType::Void),
            _ => Err(format!("Unknown parameter type: {}", s)),
        }
    }

    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x00 => Some(ContractParameterType::Any),
            0x10 => Some(ContractParameterType::Boolean),
            0x11 => Some(ContractParameterType::Integer),
            0x12 => Some(ContractParameterType::ByteArray),
            0x13 => Some(ContractParameterType::String),
            0x14 => Some(ContractParameterType::Hash160),
            0x15 => Some(ContractParameterType::Hash256),
            0x16 => Some(ContractParameterType::PublicKey),
            0x17 => Some(ContractParameterType::Signature),
            0x20 => Some(ContractParameterType::Array),
            0x22 => Some(ContractParameterType::Map),
            0x30 => Some(ContractParameterType::InteropInterface),
            0xff => Some(ContractParameterType::Void),
            _ => None,
        }
    }
}
