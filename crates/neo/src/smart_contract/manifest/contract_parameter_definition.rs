//! ContractParameterDefinition - matches C# Neo.SmartContract.Manifest.ContractParameterDefinition exactly

use crate::smart_contract::i_interoperable::IInteroperable;
use crate::smart_contract::ContractParameterType;
use neo_vm::StackItem;
use num_traits::ToPrimitive;
use serde::{Deserialize, Serialize};

/// Represents a parameter of an event or method in ABI (matches C# ContractParameterDefinition)
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractParameterDefinition {
    /// The name of the parameter
    pub name: String,

    /// The type of the parameter
    #[serde(rename = "type")]
    pub param_type: ContractParameterType,
}

impl ContractParameterDefinition {
    /// Creates a new parameter definition
    pub fn new(name: String, param_type: ContractParameterType) -> Result<Self, String> {
        if name.is_empty() {
            return Err("Parameter name cannot be empty".to_string());
        }

        if param_type == ContractParameterType::Void {
            return Err("Parameter type cannot be Void".to_string());
        }

        Ok(Self { name, param_type })
    }

    /// Creates from JSON
    pub fn from_json(json: &serde_json::Value) -> Result<Self, String> {
        let obj = json.as_object().ok_or("Expected object")?;

        let name = obj
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or("Missing name")?
            .to_string();

        let type_str = obj
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or("Missing type")?;

        let param_type = ContractParameterType::from_string(type_str)?;

        Self::new(name, param_type)
    }

    /// Converts to JSON
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "type": format!("{:?}", self.param_type),
        })
    }

    /// Approximate serialized size of the parameter definition.
    pub fn size(&self) -> usize {
        1 + self.name.len()
    }
}

impl IInteroperable for ContractParameterDefinition {
    fn from_stack_item(&mut self, stack_item: StackItem) {
        if let StackItem::Struct(struct_item) = stack_item {
            let items = struct_item.items();
            if items.len() < 2 {
                return;
            }

            if let Ok(bytes) = items[0].as_bytes() {
                if let Ok(name) = String::from_utf8(bytes) {
                    self.name = name;
                }
            }

            if let Ok(integer) = items[1].as_int() {
                if let Some(value) = integer.to_u8() {
                    self.param_type = ContractParameterType::from_byte(value)
                        .unwrap_or(ContractParameterType::Any);
                }
            }
        }
    }

    fn to_stack_item(&self) -> StackItem {
        StackItem::from_struct(vec![
            StackItem::from_byte_string(self.name.as_bytes()),
            StackItem::from_int(self.param_type as u8),
        ])
    }

    fn clone_box(&self) -> Box<dyn IInteroperable> {
        Box::new(self.clone())
    }
}
