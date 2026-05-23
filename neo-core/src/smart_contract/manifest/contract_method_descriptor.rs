//! ContractMethodDescriptor - matches C# Neo.SmartContract.Manifest.ContractMethodDescriptor exactly

use crate::error::CoreError;
use crate::neo_vm::StackItem;
use crate::smart_contract::i_interoperable::IInteroperable;
use crate::smart_contract::manifest::ContractParameterDefinition;
use crate::smart_contract::ContractParameterType;
use neo_vm_rs::StackValue;
use serde::{Deserialize, Serialize};

/// Represents a method in a smart contract ABI (matches C# ContractMethodDescriptor)

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractMethodDescriptor {
    /// The name of the method
    pub name: String,

    /// The parameters of the method
    pub parameters: Vec<ContractParameterDefinition>,

    /// The return type of the method
    #[serde(rename = "returntype")]
    pub return_type: ContractParameterType,

    /// The position of the method in the contract script
    pub offset: i32,

    /// Indicates whether the method is a safe method
    pub safe: bool,
}

impl Default for ContractMethodDescriptor {
    fn default() -> Self {
        Self {
            name: String::new(),
            parameters: Vec::new(),
            return_type: ContractParameterType::Void,
            offset: 0,
            safe: false,
        }
    }
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

    /// Approximate serialized size of the method descriptor.
    pub fn size(&self) -> usize {
        let params_size: usize = self.parameters.iter().map(|p| p.size()).sum();
        1 + self.name.len() + params_size + 1 + 4 + 1
    }

    /// Converts to a neo-vm-rs stack value (matches C# `ContractMethodDescriptor.ToStackItem` layout).
    pub fn to_stack_value(&self) -> StackValue {
        StackValue::Struct(vec![
            StackValue::ByteString(self.name.as_bytes().to_vec()),
            StackValue::Array(
                self.parameters
                    .iter()
                    .map(ContractParameterDefinition::to_stack_value)
                    .collect(),
            ),
            StackValue::Integer(self.return_type as u8 as i64),
            StackValue::Integer(i64::from(self.offset)),
            StackValue::Boolean(self.safe),
        ])
    }

    /// Updates this method descriptor from a neo-vm-rs stack value.
    pub fn from_stack_value(&mut self, stack_value: StackValue) -> Result<(), CoreError> {
        let StackValue::Struct(items) = stack_value else {
            return Err(CoreError::invalid_format(
                "ContractMethodDescriptor expects Struct stack value",
            ));
        };

        if items.len() < 5 {
            return Err(CoreError::invalid_format(format!(
                "ContractMethodDescriptor stack value must contain 5 elements, found {}",
                items.len()
            )));
        }

        if let Some(bytes) = items[0].to_byte_string_bytes() {
            if let Ok(name) = String::from_utf8(bytes) {
                self.name = name;
            }
        }

        if let StackValue::Array(param_items) | StackValue::Struct(param_items) = items[1].clone() {
            let mut params = Vec::new();
            for item in param_items {
                let mut param = ContractParameterDefinition::default();
                param.from_stack_value(item)?;
                params.push(param);
            }
            self.parameters = params;
        }

        if let Some(integer) = items[2].to_i128() {
            if let Ok(byte_val) = u8::try_from(integer) {
                self.return_type = ContractParameterType::try_from_u8(byte_val)
                    .unwrap_or(ContractParameterType::Void);
            }
        }

        if let Some(integer) = items[3].to_i128() {
            if let Ok(offset) = i32::try_from(integer) {
                self.offset = offset;
            }
        }

        self.safe = items[4].to_bool();

        Ok(())
    }
}

impl IInteroperable for ContractMethodDescriptor {
    fn from_stack_item(&mut self, stack_item: StackItem) -> Result<(), CoreError> {
        self.from_stack_value(StackValue::try_from(stack_item).map_err(|error| {
            CoreError::invalid_format(format!(
                "Failed to convert ContractMethodDescriptor StackItem to StackValue: {error}"
            ))
        })?)
    }

    fn to_stack_item(&self) -> Result<StackItem, CoreError> {
        StackItem::try_from(self.to_stack_value()).map_err(|error| {
            CoreError::invalid_operation(format!(
                "Failed to convert ContractMethodDescriptor StackValue to StackItem: {error}"
            ))
        })
    }

    fn clone_box(&self) -> Box<dyn IInteroperable> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_vm_rs::StackValue;

    fn parameter(name: &str, param_type: ContractParameterType) -> ContractParameterDefinition {
        ContractParameterDefinition::new(name.to_string(), param_type).unwrap()
    }

    #[test]
    fn method_descriptor_projects_to_neo_vm_rs_stack_value() {
        let method = ContractMethodDescriptor::new(
            "balanceOf".to_string(),
            vec![parameter("account", ContractParameterType::Hash160)],
            ContractParameterType::Integer,
            42,
            true,
        )
        .unwrap();

        assert_eq!(
            method.to_stack_value(),
            StackValue::Struct(vec![
                StackValue::ByteString(b"balanceOf".to_vec()),
                StackValue::Array(vec![StackValue::Struct(vec![
                    StackValue::ByteString(b"account".to_vec()),
                    StackValue::Integer(ContractParameterType::Hash160 as u8 as i64),
                ])]),
                StackValue::Integer(ContractParameterType::Integer as u8 as i64),
                StackValue::Integer(42),
                StackValue::Boolean(true),
            ])
        );
    }

    #[test]
    fn method_descriptor_stack_item_projection_matches_stack_value_projection() {
        let method = ContractMethodDescriptor::new(
            "transfer".to_string(),
            vec![
                parameter("to", ContractParameterType::Hash160),
                parameter("amount", ContractParameterType::Integer),
            ],
            ContractParameterType::Boolean,
            7,
            false,
        )
        .unwrap();

        let expected = StackItem::try_from(method.to_stack_value()).unwrap();
        assert_eq!(method.to_stack_item().unwrap(), expected);
    }

    #[test]
    fn method_descriptor_reads_from_neo_vm_rs_stack_value() {
        let mut method = ContractMethodDescriptor::default();

        method
            .from_stack_value(StackValue::Struct(vec![
                StackValue::ByteString(b"symbol".to_vec()),
                StackValue::Array(vec![StackValue::Struct(vec![
                    StackValue::ByteString(b"format".to_vec()),
                    StackValue::Integer(ContractParameterType::String as u8 as i64),
                ])]),
                StackValue::Integer(ContractParameterType::String as u8 as i64),
                StackValue::Integer(12),
                StackValue::Boolean(true),
            ]))
            .unwrap();

        assert_eq!(method.name, "symbol");
        assert_eq!(
            method.parameters,
            vec![parameter("format", ContractParameterType::String)]
        );
        assert_eq!(method.return_type, ContractParameterType::String);
        assert_eq!(method.offset, 12);
        assert!(method.safe);
    }

    #[test]
    fn method_descriptor_keeps_invalid_return_type_fallback() {
        let mut method = ContractMethodDescriptor::new(
            "initial".to_string(),
            Vec::new(),
            ContractParameterType::Boolean,
            1,
            false,
        )
        .unwrap();

        method
            .from_stack_value(StackValue::Struct(vec![
                StackValue::ByteString(b"changed".to_vec()),
                StackValue::Array(Vec::new()),
                StackValue::Integer(0x7f),
                StackValue::Integer(3),
                StackValue::Boolean(true),
            ]))
            .unwrap();

        assert_eq!(method.name, "changed");
        assert_eq!(method.return_type, ContractParameterType::Void);
        assert_eq!(method.offset, 3);
        assert!(method.safe);
    }
}
