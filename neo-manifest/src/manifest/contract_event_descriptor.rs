//! ContractEventDescriptor - matches C# Neo.SmartContract.Manifest.ContractEventDescriptor exactly

use crate::manifest::ContractParameterDefinition;
use crate::manifest::stack_value_helpers::{
    decode_stack_value_objects, required_struct_fields, stack_value_to_utf8_string,
};
use neo_error::{CoreError, CoreResult};
use neo_vm::Interoperable;
use neo_vm::InteroperableError;
use neo_vm_rs::StackValue;
use serde::{Deserialize, Serialize};

/// Represents an event in a smart contract ABI (matches C# ContractEventDescriptor)
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractEventDescriptor {
    /// The name of the event
    pub name: String,

    /// The parameters of the event
    pub parameters: Vec<ContractParameterDefinition>,
}

impl ContractEventDescriptor {
    /// Creates a new event descriptor
    pub fn new(name: String, parameters: Vec<ContractParameterDefinition>) -> CoreResult<Self> {
        if name.is_empty() {
            return Err(CoreError::other("Event name cannot be empty"));
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

        Ok(Self { name, parameters })
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

        Self::new(name, parameters)
    }

    /// Converts to JSON
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "parameters": self.parameters.iter().map(|p| p.to_json()).collect::<Vec<_>>(),
        })
    }

    /// Approximate serialized size of the event descriptor.
    pub fn size(&self) -> usize {
        let params_size: usize = self.parameters.iter().map(|p| p.size()).sum();
        1 + self.name.len() + params_size
    }

    /// Converts to a neo-vm-rs stack value (matches C# `ContractEventDescriptor.ToStackItem` layout).
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
            ],
        )
    }

    /// Updates this event descriptor from a neo-vm-rs stack value.
    pub fn from_stack_value(&mut self, stack_value: StackValue) -> Result<(), CoreError> {
        let items = required_struct_fields(stack_value, "ContractEventDescriptor", 2)?;

        self.name = stack_value_to_utf8_string(&items[0], "ContractEventDescriptor name")?;

        self.parameters = decode_stack_value_objects(
            items[1].clone(),
            ContractParameterDefinition::from_stack_value,
        )?;

        Ok(())
    }
}

impl Interoperable for ContractEventDescriptor {
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
    use neo_primitives::ContractParameterType;
    use neo_vm_rs::StackValue;

    fn parameter(name: &str, param_type: ContractParameterType) -> ContractParameterDefinition {
        ContractParameterDefinition::new(name.to_string(), param_type).unwrap()
    }

    #[test]
    fn event_descriptor_projects_to_neo_vm_rs_stack_value() {
        let event = ContractEventDescriptor::new(
            "Transfer".to_string(),
            vec![
                parameter("from", ContractParameterType::Hash160),
                parameter("amount", ContractParameterType::Integer),
            ],
        )
        .unwrap();

        assert_eq!(
            event.to_stack_value(),
            StackValue::Struct(
                0,
                vec![
                    StackValue::ByteString(b"Transfer".to_vec()),
                    StackValue::Array(
                        0,
                        vec![
                            StackValue::Struct(
                                0,
                                vec![
                                    StackValue::ByteString(b"from".to_vec()),
                                    StackValue::Integer(
                                        ContractParameterType::Hash160 as u8 as i64
                                    ),
                                ]
                            ),
                            StackValue::Struct(
                                0,
                                vec![
                                    StackValue::ByteString(b"amount".to_vec()),
                                    StackValue::Integer(
                                        ContractParameterType::Integer as u8 as i64
                                    ),
                                ]
                            ),
                        ]
                    ),
                ]
            )
        );
    }

    #[test]
    fn event_descriptor_reads_from_neo_vm_rs_stack_value() {
        let mut event = ContractEventDescriptor::default();

        event
            .from_stack_value(StackValue::Struct(
                0,
                vec![
                    StackValue::ByteString(b"Approval".to_vec()),
                    StackValue::Array(
                        0,
                        vec![StackValue::Struct(
                            0,
                            vec![
                                StackValue::ByteString(b"spender".to_vec()),
                                StackValue::Integer(ContractParameterType::Hash160 as u8 as i64),
                            ],
                        )],
                    ),
                ],
            ))
            .unwrap();

        assert_eq!(event.name, "Approval");
        assert_eq!(
            event.parameters,
            vec![parameter("spender", ContractParameterType::Hash160)]
        );
    }

    #[test]
    fn event_descriptor_rejects_struct_parameter_sequence_like_csharp() {
        let mut event = ContractEventDescriptor::default();

        assert!(
            event
                .from_stack_value(StackValue::Struct(
                    0,
                    vec![
                        StackValue::ByteString(b"Vote".to_vec()),
                        StackValue::Struct(
                            0,
                            vec![StackValue::Struct(
                                0,
                                vec![
                                    StackValue::ByteString(b"candidate".to_vec()),
                                    StackValue::Integer(
                                        ContractParameterType::PublicKey as u8 as i64
                                    ),
                                ]
                            )]
                        ),
                    ]
                ))
                .is_err()
        );
    }

    #[test]
    fn event_descriptor_rejects_invalid_name_like_csharp() {
        let mut event = ContractEventDescriptor::default();

        assert!(
            event
                .from_stack_value(StackValue::Struct(
                    0,
                    vec![StackValue::Null, StackValue::Array(0, Vec::new()),]
                ))
                .is_err()
        );
        assert!(
            event
                .from_stack_value(StackValue::Struct(
                    0,
                    vec![
                        StackValue::ByteString(vec![0xff]),
                        StackValue::Array(0, Vec::new()),
                    ]
                ))
                .is_err()
        );
    }

    #[test]
    fn event_descriptor_from_json_rejects_missing_or_invalid_parameters_like_csharp() {
        let missing_parameters = serde_json::json!({
            "name": "Notify"
        });
        assert!(ContractEventDescriptor::from_json(&missing_parameters).is_err());

        let invalid_parameter = serde_json::json!({
            "name": "Notify",
            "parameters": [{"name": "bad", "type": "Void"}]
        });
        assert!(ContractEventDescriptor::from_json(&invalid_parameter).is_err());
    }
}
