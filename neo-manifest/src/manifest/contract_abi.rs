//! ContractAbi - matches C# Neo.SmartContract.Manifest.ContractAbi exactly

use crate::manifest::stack_value_helpers::{decode_stack_value_objects, required_struct_fields};
use crate::manifest::{ContractEventDescriptor, ContractMethodDescriptor};
use neo_error::CoreError;
use neo_vm::Interoperable;
use neo_vm::StackItem;
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
    pub fn from_json(json: &serde_json::Value) -> Result<Self, String> {
        let obj = json.as_object().ok_or("Expected object")?;

        let methods: Vec<ContractMethodDescriptor> = obj
            .get("methods")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| ContractMethodDescriptor::from_json(m).ok())
                    .collect()
            })
            .unwrap_or_default();

        let events: Vec<ContractEventDescriptor> = obj
            .get("events")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|e| ContractEventDescriptor::from_json(e).ok())
                    .collect()
            })
            .unwrap_or_default();

        if methods.is_empty() {
            return Err("ABI must have at least one method".to_string());
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
        StackValue::Struct(vec![
            StackValue::Array(
                self.methods
                    .iter()
                    .map(ContractMethodDescriptor::to_stack_value)
                    .collect(),
            ),
            StackValue::Array(
                self.events
                    .iter()
                    .map(ContractEventDescriptor::to_stack_value)
                    .collect(),
            ),
        ])
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

        if let Some(methods) = methods {
            self.methods = methods;
        }
        if let Some(events) = events {
            self.events = events;
        }
        self.method_dictionary = None;

        Ok(())
    }

    /// Validates the ABI structure.
    pub fn validate(&self) -> Result<(), String> {
        if self.methods.is_empty() {
            return Err("ABI must contain at least one method".to_string());
        }
        Ok(())
    }
}

impl Interoperable for ContractAbi {
    fn from_stack_item(&mut self, stack_item: StackItem) -> Result<(), neo_vm::VmError> {
        let sv = StackValue::try_from(stack_item).map_err(|error| {
            neo_vm::VmError::invalid_operation_msg(format!(
                "Failed to convert ContractAbi StackItem to StackValue: {error}"
            ))
        })?;
        self.from_stack_value(sv)
            .map_err(|e| neo_vm::VmError::invalid_operation_msg(e.to_string()))
    }

    fn to_stack_item(&self) -> Result<StackItem, neo_vm::VmError> {
        StackItem::try_from(self.to_stack_value()).map_err(|error| {
            neo_vm::VmError::invalid_operation_msg(format!(
                "Failed to convert ContractAbi StackValue to StackItem: {error}"
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
            StackValue::Struct(vec![
                StackValue::Array(vec![StackValue::Struct(vec![
                    StackValue::ByteString(b"main".to_vec()),
                    StackValue::Array(Vec::new()),
                    StackValue::Integer(ContractParameterType::Void as u8 as i64),
                    StackValue::Integer(7),
                    StackValue::Boolean(true),
                ])]),
                StackValue::Array(vec![StackValue::Struct(vec![
                    StackValue::ByteString(b"Notify".to_vec()),
                    StackValue::Array(Vec::new()),
                ])]),
            ])
        );
    }

    #[test]
    fn contract_abi_stack_item_projection_matches_stack_value_projection() {
        let abi = ContractAbi::new(vec![method("transfer")], Vec::new());
        let expected = StackItem::try_from(abi.to_stack_value()).unwrap();

        assert_eq!(abi.to_stack_item().unwrap(), expected);
    }

    #[test]
    fn contract_abi_reads_from_neo_vm_rs_stack_value_and_clears_method_cache() {
        let mut abi = ContractAbi::new(vec![method("old")], Vec::new());
        assert!(abi.get_method("old", 0).is_some());

        abi.from_stack_value(StackValue::Struct(vec![
            StackValue::Array(vec![method("new").to_stack_value()]),
            StackValue::Array(vec![event("Updated").to_stack_value()]),
        ]))
        .unwrap();

        assert!(abi.get_method("old", 0).is_none());
        assert!(abi.get_method("new", 0).is_some());
        assert_eq!(abi.events, vec![event("Updated")]);
    }

    #[test]
    fn contract_abi_reads_struct_sequences_from_neo_vm_rs_stack_value() {
        let mut abi = ContractAbi::default();

        abi.from_stack_value(StackValue::Struct(vec![
            StackValue::Struct(vec![method("main").to_stack_value()]),
            StackValue::Struct(vec![event("Notify").to_stack_value()]),
        ]))
        .unwrap();

        assert_eq!(abi.methods, vec![method("main")]);
        assert_eq!(abi.events, vec![event("Notify")]);
    }
}
