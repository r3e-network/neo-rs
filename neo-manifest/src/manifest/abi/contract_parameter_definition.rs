//! ContractParameterDefinition - matches C# Neo.SmartContract.Manifest.ContractParameterDefinition exactly

use neo_error::{CoreError, CoreResult};
use neo_primitives::ContractParameterType;
use neo_vm::Interoperable;
use neo_vm::InteroperableError;
use neo_vm_rs::StackValue;
use serde::{Deserialize, Serialize};

use crate::manifest::stack_value_helpers::{
    json_string_to_parameter_type, stack_value_to_parameter_type, stack_value_to_utf8_string,
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

    /// Converts to a neo-vm-rs stack value (matches C# `ContractParameterDefinition.ToStackItem` layout).
    pub fn to_stack_value(&self) -> StackValue {
        StackValue::Struct(
            neo_vm_rs::next_stack_item_id(),
            vec![
                StackValue::ByteString(self.name.as_bytes().to_vec()),
                StackValue::Integer(self.param_type as u8 as i64),
            ],
        )
    }

    /// Updates this definition from a neo-vm-rs stack value.
    pub fn from_stack_value(&mut self, stack_value: StackValue) -> Result<(), CoreError> {
        let StackValue::Struct(_, items) = stack_value else {
            return Err(CoreError::invalid_format(
                "ContractParameterDefinition expects Struct stack value",
            ));
        };

        if items.len() < 2 {
            return Err(CoreError::invalid_format(format!(
                "ContractParameterDefinition stack value must contain 2 elements, found {}",
                items.len()
            )));
        }

        self.name = stack_value_to_utf8_string(&items[0], "ContractParameterDefinition name")?;
        self.param_type =
            stack_value_to_parameter_type(&items[1], "ContractParameterDefinition type")?;

        Ok(())
    }
}

impl Interoperable for ContractParameterDefinition {
    fn from_stack_value(&mut self, value: StackValue) -> Result<(), InteroperableError> {
        self.from_stack_value(value)
            .map_err(|e| InteroperableError::InvalidData(e.to_string()))
    }

    fn to_stack_value(&self) -> Result<StackValue, InteroperableError> {
        Ok(self.to_stack_value())
    }
}

#[cfg(test)]
#[path = "../../tests/manifest/contract_parameter_definition.rs"]
mod tests;
