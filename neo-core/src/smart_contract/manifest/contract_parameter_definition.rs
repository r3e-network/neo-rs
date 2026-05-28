//! ContractParameterDefinition - matches C# Neo.SmartContract.Manifest.ContractParameterDefinition exactly

use crate::error::CoreError;
use crate::smart_contract::interoperable::Interoperable;
use crate::smart_contract::ContractParameterType;
use crate::neo_vm::StackItem;
use neo_vm_rs::StackValue;
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

    /// Converts to a neo-vm-rs stack value (matches C# `ContractParameterDefinition.ToStackItem` layout).
    pub fn to_stack_value(&self) -> StackValue {
        StackValue::Struct(vec![
            StackValue::ByteString(self.name.as_bytes().to_vec()),
            StackValue::Integer(self.param_type as u8 as i64),
        ])
    }

    /// Updates this definition from a neo-vm-rs stack value.
    pub fn from_stack_value(&mut self, stack_value: StackValue) -> Result<(), CoreError> {
        let StackValue::Struct(items) = stack_value else {
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

        if let Some(bytes) = items[0].to_byte_string_bytes() {
            if let Ok(name) = String::from_utf8(bytes) {
                self.name = name;
            }
        }

        if let Some(integer) = items[1].to_i128() {
            if let Ok(value) = u8::try_from(integer) {
                self.param_type =
                    ContractParameterType::from_byte(value).unwrap_or(ContractParameterType::Any);
            }
        }

        Ok(())
    }
}

impl Interoperable for ContractParameterDefinition {
    fn from_stack_item(&mut self, stack_item: StackItem) -> Result<(), crate::neo_vm::VmError> {
        let sv = StackValue::try_from(stack_item).map_err(|error| {
            crate::neo_vm::VmError::invalid_operation_msg(format!(
                "Failed to convert ContractParameterDefinition StackItem to StackValue: {error}"
            ))
        })?;
        self.from_stack_value(sv).map_err(|e| crate::neo_vm::VmError::invalid_operation_msg(e.to_string()))
    }

    fn to_stack_item(&self) -> Result<StackItem, crate::neo_vm::VmError> {
        StackItem::try_from(self.to_stack_value()).map_err(|error| {
            crate::neo_vm::VmError::invalid_operation_msg(format!(
                "Failed to convert ContractParameterDefinition StackValue to StackItem: {error}"
            ))
        })
    }

    fn clone_box(&self) -> Box<dyn Interoperable> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_vm_rs::StackValue;

    #[test]
    fn parameter_definition_projects_to_neo_vm_rs_stack_value() {
        let definition =
            ContractParameterDefinition::new("owner".to_string(), ContractParameterType::Hash160)
                .unwrap();

        assert_eq!(
            definition.to_stack_value(),
            StackValue::Struct(vec![
                StackValue::ByteString(b"owner".to_vec()),
                StackValue::Integer(ContractParameterType::Hash160 as u8 as i64),
            ])
        );
    }

    #[test]
    fn parameter_definition_stack_item_projection_matches_stack_value_projection() {
        let definition =
            ContractParameterDefinition::new("amount".to_string(), ContractParameterType::Integer)
                .unwrap();

        let expected = StackItem::try_from(definition.to_stack_value()).unwrap();
        assert_eq!(definition.to_stack_item().unwrap(), expected);
    }

    #[test]
    fn parameter_definition_reads_from_neo_vm_rs_stack_value() {
        let mut definition = ContractParameterDefinition::default();

        definition
            .from_stack_value(StackValue::Struct(vec![
                StackValue::ByteString(b"flag".to_vec()),
                StackValue::Integer(ContractParameterType::Boolean as u8 as i64),
            ]))
            .unwrap();

        assert_eq!(definition.name, "flag");
        assert_eq!(definition.param_type, ContractParameterType::Boolean);
    }

    #[test]
    fn parameter_definition_keeps_invalid_type_fallback() {
        let mut definition =
            ContractParameterDefinition::new("initial".to_string(), ContractParameterType::String)
                .unwrap();

        definition
            .from_stack_value(StackValue::Struct(vec![
                StackValue::ByteString(b"changed".to_vec()),
                StackValue::Integer(0x7f),
            ]))
            .unwrap();

        assert_eq!(definition.name, "changed");
        assert_eq!(definition.param_type, ContractParameterType::Any);
    }
}
