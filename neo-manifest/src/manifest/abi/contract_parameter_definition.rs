//! ContractParameterDefinition - matches C# Neo.SmartContract.Manifest.ContractParameterDefinition exactly

use neo_error::{CoreError, CoreResult};
use neo_primitives::ContractParameterType;
use neo_vm::Interoperable;
use neo_vm::InteroperableError;
use neo_vm::StackItem;
use serde::{Deserialize, Serialize};

use crate::manifest::stack_item_helpers::{
    json_string_to_parameter_type, stack_item_to_parameter_type, stack_item_to_utf8_string,
};

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
    pub fn new(name: String, param_type: ContractParameterType) -> CoreResult<Self> {
        if name.is_empty() {
            return Err(CoreError::other("Parameter name cannot be empty"));
        }

        if param_type == ContractParameterType::Void {
            return Err(CoreError::other("Parameter type cannot be Void"));
        }

        Ok(Self { name, param_type })
    }

    /// Creates from JSON
    pub fn from_json(json: &serde_json::Value) -> CoreResult<Self> {
        let obj = json
            .as_object()
            .ok_or_else(|| CoreError::other("Expected object"))?;

        let name = obj
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CoreError::other("Missing name"))?
            .to_string();

        let type_str = obj
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CoreError::other("Missing type"))?;

        let param_type =
            json_string_to_parameter_type(type_str, "ContractParameterDefinition type")?;

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

    /// Converts to a neo-vm stack item (matches C# `ContractParameterDefinition.ToStackItem` layout).
    pub fn to_stack_item(&self) -> StackItem {
        StackItem::from_struct(vec![
            StackItem::from_byte_string(self.name.as_bytes().to_vec()),
            StackItem::from_i64(self.param_type as u8 as i64),
        ])
    }

    /// Updates this definition from a neo-vm stack item.
    pub fn from_stack_item(&mut self, stack_item: StackItem) -> Result<(), CoreError> {
        let StackItem::Struct(structure) = stack_item else {
            return Err(CoreError::invalid_format(
                "ContractParameterDefinition expects Struct stack item",
            ));
        };
        let items = structure.items();

        if items.len() < 2 {
            return Err(CoreError::invalid_format(format!(
                "ContractParameterDefinition stack item must contain 2 elements, found {}",
                items.len()
            )));
        }

        self.name = stack_item_to_utf8_string(&items[0], "ContractParameterDefinition name")?;
        self.param_type =
            stack_item_to_parameter_type(&items[1], "ContractParameterDefinition type")?;

        Ok(())
    }
}

impl Interoperable for ContractParameterDefinition {
    fn from_stack_item(&mut self, value: StackItem) -> Result<(), InteroperableError> {
        self.from_stack_item(value)
            .map_err(|e| InteroperableError::InvalidData(e.to_string()))
    }

    fn to_stack_item(&self) -> Result<StackItem, InteroperableError> {
        Ok(self.to_stack_item())
    }
}

#[cfg(test)]
#[path = "../../tests/manifest/contract_parameter_definition.rs"]
mod tests;
