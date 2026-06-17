//! ContractAbi - matches C# Neo.SmartContract.Manifest.ContractAbi exactly

use crate::manifest::stack_value_helpers::{decode_stack_value_objects, required_struct_fields};
use crate::manifest::{ContractEventDescriptor, ContractMethodDescriptor};
use neo_error::{CoreError, CoreResult};
use neo_vm::Interoperable;
use neo_vm::InteroperableError;
use neo_vm_rs::StackValue;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents the ABI of a smart contract (matches C# ContractAbi)
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ContractAbi {
    /// The methods in the ABI
    pub methods: Vec<ContractMethodDescriptor>,

    /// The events in the ABI
    pub events: Vec<ContractEventDescriptor>,

    #[serde(skip)]
    method_dictionary: Option<HashMap<(String, i32), usize>>,
}

impl ContractAbi {
    /// Creates a new ContractAbi
    pub fn new(
        methods: Vec<ContractMethodDescriptor>,
        events: Vec<ContractEventDescriptor>,
    ) -> Self {
        Self {
            methods,
            events,
            method_dictionary: None,
        }
    }

    /// Gets the method with the specified name and parameter count.
    ///
    /// Mirrors C# `ContractAbi.GetMethod`: `pcount < 0` returns the FIRST
    /// method with a matching name (`Methods.FirstOrDefault`), regardless of
    /// parameter count — used for `verify` resolution. For `pcount >= 0` the
    /// lookup is keyed by `(name, parameter_count)`.
    pub fn get_method(&mut self, name: &str, pcount: i32) -> Option<&ContractMethodDescriptor> {
        if pcount < 0 {
            return self.methods.iter().find(|method| method.name == name);
        }

        // Build the (name, parameter_count) dictionary on demand.
        if self.method_dictionary.is_none() {
            let mut dict = HashMap::new();
            for (i, method) in self.methods.iter().enumerate() {
                dict.insert((method.name.clone(), method.parameters.len() as i32), i);
            }
            self.method_dictionary = Some(dict);
        }

        if let Some(dict) = &self.method_dictionary {
            if let Some(&index) = dict.get(&(name.to_string(), pcount)) {
                return self.methods.get(index);
            }
        }

        None
    }

    /// Gets the method with the specified name and parameter count without modifying the ABI cache.
    pub fn get_method_ref(
        &self,
        name: &str,
        parameter_count: usize,
    ) -> Option<&ContractMethodDescriptor> {
        self.methods.iter().find(|method| {
            if method.name != name {
                return false;
            }

            if parameter_count == method.parameters.len() {
                return true;
            }

            if parameter_count == 0 && method.parameters.is_empty() {
                return true;
            }

            false
        })
    }

    /// Creates from JSON
    pub fn from_json(json: &serde_json::Value) -> CoreResult<Self> {
        let obj = json
            .as_object()
            .ok_or_else(|| CoreError::other("Expected object"))?;

        let methods: Vec<ContractMethodDescriptor> = match obj.get("methods") {
            Some(serde_json::Value::Array(arr)) => arr
                .iter()
                .map(ContractMethodDescriptor::from_json)
                .collect::<CoreResult<Vec<_>>>()?,
            Some(_) => return Err(CoreError::other("ContractAbi methods must be an array")),
            None => Vec::new(),
        };

        let events: Vec<ContractEventDescriptor> = match obj.get("events") {
            Some(serde_json::Value::Array(arr)) => arr
                .iter()
                .map(ContractEventDescriptor::from_json)
                .collect::<CoreResult<Vec<_>>>()?,
            Some(_) => return Err(CoreError::other("ContractAbi events must be an array")),
            None => Vec::new(),
        };

        if methods.is_empty() {
            return Err(CoreError::other("ABI must have at least one method"));
        }

        Ok(Self::new(methods, events))
    }

    /// Converts to JSON
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "methods": self.methods.iter().map(|m| m.to_json()).collect::<Vec<_>>(),
            "events": self.events.iter().map(|e| e.to_json()).collect::<Vec<_>>(),
        })
    }

    /// Approximate serialized size of the ABI.
    pub fn size(&self) -> usize {
        let methods_size: usize = self.methods.iter().map(|m| m.size()).sum();
        let events_size: usize = self.events.iter().map(|e| e.size()).sum();
        methods_size + events_size
    }

    /// Converts to a neo-vm-rs stack value (matches C# `ContractAbi.ToStackItem` layout).
    pub fn to_stack_value(&self) -> StackValue {
        StackValue::Struct(
            0,
            vec![
                StackValue::Array(
                    0,
                    self.methods
                        .iter()
                        .map(ContractMethodDescriptor::to_stack_value)
                        .collect(),
                ),
                StackValue::Array(
                    0,
                    self.events
                        .iter()
                        .map(ContractEventDescriptor::to_stack_value)
                        .collect(),
                ),
            ],
        )
    }

    /// Updates this ABI from a neo-vm-rs stack value.
    pub fn from_stack_value(&mut self, stack_value: StackValue) -> Result<(), CoreError> {
        let items = required_struct_fields(stack_value, "ContractAbi", 2)?;

        let methods = decode_stack_value_objects(
            items[0].clone(),
            ContractMethodDescriptor::from_stack_value,
        )?;
        let events = decode_stack_value_objects(
            items[1].clone(),
            ContractEventDescriptor::from_stack_value,
        )?;

        self.methods = methods;
        self.events = events;
        self.method_dictionary = None;

        Ok(())
    }

    /// Validates the ABI structure.
    pub fn validate(&self) -> CoreResult<()> {
        if self.methods.is_empty() {
            return Err(CoreError::other("ABI must contain at least one method"));
        }
        Ok(())
    }
}

impl Interoperable for ContractAbi {
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

    fn method(name: &str) -> ContractMethodDescriptor {
        ContractMethodDescriptor::new(
            name.to_string(),
            Vec::new(),
            ContractParameterType::Void,
            7,
            true,
        )
        .unwrap()
    }

    fn event(name: &str) -> ContractEventDescriptor {
        ContractEventDescriptor::new(name.to_string(), Vec::new()).unwrap()
    }

    #[test]
    fn contract_abi_projects_to_neo_vm_rs_stack_value() {
        let abi = ContractAbi::new(vec![method("main")], vec![event("Notify")]);

        assert_eq!(
            abi.to_stack_value(),
            StackValue::Struct(
                0,
                vec![
                    StackValue::Array(
                        0,
                        vec![StackValue::Struct(
                            0,
                            vec![
                                StackValue::ByteString(b"main".to_vec()),
                                StackValue::Array(0, Vec::new()),
                                StackValue::Integer(ContractParameterType::Void as u8 as i64),
                                StackValue::Integer(7),
                                StackValue::Boolean(true),
                            ]
                        )]
                    ),
                    StackValue::Array(
                        0,
                        vec![StackValue::Struct(
                            0,
                            vec![
                                StackValue::ByteString(b"Notify".to_vec()),
                                StackValue::Array(0, Vec::new()),
                            ]
                        )]
                    ),
                ]
            )
        );
    }

    #[test]
    fn contract_abi_reads_from_neo_vm_rs_stack_value_and_clears_method_cache() {
        let mut abi = ContractAbi::new(vec![method("old")], Vec::new());
        assert!(abi.get_method("old", 0).is_some());

        abi.from_stack_value(StackValue::Struct(
            0,
            vec![
                StackValue::Array(0, vec![method("new").to_stack_value()]),
                StackValue::Array(0, vec![event("Updated").to_stack_value()]),
            ],
        ))
        .unwrap();

        assert!(abi.get_method("old", 0).is_none());
        assert!(abi.get_method("new", 0).is_some());
        assert_eq!(abi.events, vec![event("Updated")]);
    }

    #[test]
    fn contract_abi_rejects_struct_sequences_like_csharp() {
        let mut abi = ContractAbi::default();

        assert!(
            abi.from_stack_value(StackValue::Struct(
                0,
                vec![
                    StackValue::Struct(0, vec![method("main").to_stack_value()]),
                    StackValue::Array(0, vec![event("Notify").to_stack_value()]),
                ]
            ))
            .is_err()
        );
        assert!(
            abi.from_stack_value(StackValue::Struct(
                0,
                vec![
                    StackValue::Array(0, vec![method("main").to_stack_value()]),
                    StackValue::Struct(0, vec![event("Notify").to_stack_value()]),
                ]
            ))
            .is_err()
        );
    }

    #[test]
    fn contract_abi_from_json_rejects_malformed_children_like_csharp() {
        let invalid_method = serde_json::json!({
            "methods": [{
                "name": "broken",
                "parameters": [{"name": "bad", "type": "Void"}],
                "returntype": "Void",
                "offset": 0,
                "safe": false
            }],
            "events": []
        });
        assert!(ContractAbi::from_json(&invalid_method).is_err());

        let invalid_event = serde_json::json!({
            "methods": [{
                "name": "main",
                "parameters": [],
                "returntype": "Void",
                "offset": 0,
                "safe": false
            }],
            "events": [{
                "name": "Notify",
                "parameters": [{"name": "", "type": "String"}]
            }]
        });
        assert!(ContractAbi::from_json(&invalid_event).is_err());

        let non_array = serde_json::json!({
            "methods": {},
            "events": []
        });
        assert!(ContractAbi::from_json(&non_array).is_err());
    }
}
