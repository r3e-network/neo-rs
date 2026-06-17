//! ContractMethodDescriptor - matches C# Neo.SmartContract.Manifest.ContractMethodDescriptor exactly

use crate::manifest::ContractParameterDefinition;
use crate::manifest::stack_value_helpers::{
    decode_stack_value_objects, json_string_to_parameter_type, required_struct_fields,
    stack_value_to_i32, stack_value_to_parameter_type, stack_value_to_utf8_string,
};
use neo_error::{CoreError, CoreResult};
use neo_primitives::ContractParameterType;
use neo_vm::Interoperable;
use neo_vm::InteroperableError;
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
    ) -> CoreResult<Self> {
        if name.is_empty() {
            return Err(CoreError::other("Method name cannot be empty"));
        }

        if offset < 0 {
            return Err(CoreError::other("Offset cannot be negative"));
        }

        // Check for duplicate parameter names
        let mut names = std::collections::HashSet::new();
        for param in &parameters {
            if !names.insert(&param.name) {
                return Err(CoreError::other(format!(
                    "Duplicate parameter name: {}",
                    param.name
                )));
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
    pub fn from_json(json: &serde_json::Value) -> CoreResult<Self> {
        let obj = json
            .as_object()
            .ok_or_else(|| CoreError::other("Expected object"))?;

        let name = obj
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CoreError::other("Missing name"))?
            .to_string();

        let parameters = obj
            .get("parameters")
            .and_then(|v| v.as_array())
            .ok_or_else(|| CoreError::other("Missing parameters"))?
            .iter()
            .map(ContractParameterDefinition::from_json)
            .collect::<CoreResult<Vec<_>>>()?;

        let return_type = obj
            .get("returntype")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CoreError::other("Missing returntype"))
            .and_then(|s| {
                json_string_to_parameter_type(s, "ContractMethodDescriptor return type")
            })?;

        let offset = obj
            .get("offset")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| CoreError::other("Missing offset"))
            .and_then(|value| {
                i32::try_from(value).map_err(|_| CoreError::other("Offset out of Int32 range"))
            })?;

        let safe = obj
            .get("safe")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| CoreError::other("Missing safe"))?;

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
        StackValue::Struct(
            0,
            vec![
                StackValue::ByteString(self.name.as_bytes().to_vec()),
                StackValue::Array(
                    0,
                    self.parameters
                        .iter()
                        .map(ContractParameterDefinition::to_stack_value)
                        .collect(),
                ),
                StackValue::Integer(self.return_type as u8 as i64),
                StackValue::Integer(i64::from(self.offset)),
                StackValue::Boolean(self.safe),
            ],
        )
    }

    /// Updates this method descriptor from a neo-vm-rs stack value.
    pub fn from_stack_value(&mut self, stack_value: StackValue) -> Result<(), CoreError> {
        let items = required_struct_fields(stack_value, "ContractMethodDescriptor", 5)?;

        self.name = stack_value_to_utf8_string(&items[0], "ContractMethodDescriptor name")?;

        self.parameters = decode_stack_value_objects(
            items[1].clone(),
            ContractParameterDefinition::from_stack_value,
        )?;

        self.return_type =
            stack_value_to_parameter_type(&items[2], "ContractMethodDescriptor return type")?;
        self.offset = stack_value_to_i32(&items[3], "ContractMethodDescriptor offset")?;

        self.safe = items[4].to_bool();

        Ok(())
    }
}

impl Interoperable for ContractMethodDescriptor {
    fn from_stack_value(&mut self, value: StackValue) -> Result<(), InteroperableError> {
        self.from_stack_value(value)
            .map_err(|e| InteroperableError::InvalidData(e.to_string()))
    }

    fn to_stack_value(&self) -> Result<StackValue, InteroperableError> {
        Ok(self.to_stack_value())
    }

    fn clone_box(&self) -> Box<dyn Interoperable> {
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
            StackValue::Struct(
                0,
                vec![
                    StackValue::ByteString(b"balanceOf".to_vec()),
                    StackValue::Array(
                        0,
                        vec![StackValue::Struct(
                            0,
                            vec![
                                StackValue::ByteString(b"account".to_vec()),
                                StackValue::Integer(ContractParameterType::Hash160 as u8 as i64),
                            ]
                        )]
                    ),
                    StackValue::Integer(ContractParameterType::Integer as u8 as i64),
                    StackValue::Integer(42),
                    StackValue::Boolean(true),
                ]
            )
        );
    }

    #[test]
    fn method_descriptor_reads_from_neo_vm_rs_stack_value() {
        let mut method = ContractMethodDescriptor::default();

        method
            .from_stack_value(StackValue::Struct(
                0,
                vec![
                    StackValue::ByteString(b"symbol".to_vec()),
                    StackValue::Array(
                        0,
                        vec![StackValue::Struct(
                            0,
                            vec![
                                StackValue::ByteString(b"format".to_vec()),
                                StackValue::Integer(ContractParameterType::String as u8 as i64),
                            ],
                        )],
                    ),
                    StackValue::Integer(ContractParameterType::String as u8 as i64),
                    StackValue::Integer(12),
                    StackValue::Boolean(true),
                ],
            ))
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
    fn method_descriptor_rejects_struct_parameter_sequence_like_csharp() {
        let mut method = ContractMethodDescriptor::default();

        assert!(
            method
                .from_stack_value(StackValue::Struct(
                    0,
                    vec![
                        StackValue::ByteString(b"verify".to_vec()),
                        StackValue::Struct(
                            0,
                            vec![StackValue::Struct(
                                0,
                                vec![
                                    StackValue::ByteString(b"signature".to_vec()),
                                    StackValue::Integer(
                                        ContractParameterType::Signature as u8 as i64
                                    ),
                                ]
                            )]
                        ),
                        StackValue::Integer(ContractParameterType::Boolean as u8 as i64),
                        StackValue::Integer(5),
                        StackValue::Boolean(false),
                    ]
                ))
                .is_err()
        );
    }

    #[test]
    fn method_descriptor_rejects_invalid_stack_fields_like_csharp() {
        let mut method = ContractMethodDescriptor::new(
            "initial".to_string(),
            Vec::new(),
            ContractParameterType::Boolean,
            1,
            false,
        )
        .unwrap();

        assert!(
            method
                .from_stack_value(StackValue::Struct(
                    0,
                    vec![
                        StackValue::ByteString(b"changed".to_vec()),
                        StackValue::Array(0, Vec::new()),
                        StackValue::Integer(0x7f),
                        StackValue::Integer(3),
                        StackValue::Boolean(true),
                    ]
                ))
                .is_err()
        );
        assert!(
            method
                .from_stack_value(StackValue::Struct(
                    0,
                    vec![
                        StackValue::Null,
                        StackValue::Array(0, Vec::new()),
                        StackValue::Integer(ContractParameterType::Boolean as u8 as i64),
                        StackValue::Integer(3),
                        StackValue::Boolean(true),
                    ]
                ))
                .is_err()
        );
        assert!(
            method
                .from_stack_value(StackValue::Struct(
                    0,
                    vec![
                        StackValue::ByteString(vec![0xff]),
                        StackValue::Array(0, Vec::new()),
                        StackValue::Integer(ContractParameterType::Boolean as u8 as i64),
                        StackValue::Integer(3),
                        StackValue::Boolean(true),
                    ]
                ))
                .is_err()
        );
        assert!(
            method
                .from_stack_value(StackValue::Struct(
                    0,
                    vec![
                        StackValue::ByteString(b"changed".to_vec()),
                        StackValue::Array(0, Vec::new()),
                        StackValue::Integer(ContractParameterType::Boolean as u8 as i64),
                        StackValue::Integer(i64::MAX),
                        StackValue::Boolean(true),
                    ]
                ))
                .is_err()
        );
    }

    #[test]
    fn method_descriptor_from_json_rejects_missing_or_invalid_fields_like_csharp() {
        let missing_return_type = serde_json::json!({
            "name": "main",
            "parameters": [],
            "offset": 0,
            "safe": false
        });
        assert!(ContractMethodDescriptor::from_json(&missing_return_type).is_err());

        let invalid_parameter = serde_json::json!({
            "name": "main",
            "parameters": [{"name": "bad", "type": "Void"}],
            "returntype": "Void",
            "offset": 0,
            "safe": false
        });
        assert!(ContractMethodDescriptor::from_json(&invalid_parameter).is_err());

        let overflowing_offset = serde_json::json!({
            "name": "main",
            "parameters": [],
            "returntype": "Void",
            "offset": i64::from(i32::MAX) + 1,
            "safe": false
        });
        assert!(ContractMethodDescriptor::from_json(&overflowing_offset).is_err());

        let alias_return_type = serde_json::json!({
            "name": "main",
            "parameters": [],
            "returntype": "INT",
            "offset": 0,
            "safe": false
        });
        assert!(ContractMethodDescriptor::from_json(&alias_return_type).is_err());
    }

    #[test]
    fn method_descriptor_from_json_accepts_csharp_numeric_return_type() {
        let numeric_void = serde_json::json!({
            "name": "main",
            "parameters": [],
            "returntype": "255",
            "offset": 0,
            "safe": false
        });
        let method = ContractMethodDescriptor::from_json(&numeric_void).unwrap();
        assert_eq!(method.return_type, ContractParameterType::Void);
    }
}
