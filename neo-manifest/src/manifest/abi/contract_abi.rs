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
            neo_vm_rs::next_stack_item_id(),
            vec![
                StackValue::Array(
                    neo_vm_rs::next_stack_item_id(),
                    self.methods
                        .iter()
                        .map(ContractMethodDescriptor::to_stack_value)
                        .collect(),
                ),
                StackValue::Array(
                    neo_vm_rs::next_stack_item_id(),
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
}

#[cfg(test)]
#[path = "../../tests/manifest/contract_abi.rs"]
mod tests;
