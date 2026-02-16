//! ContractAbi - matches C# Neo.SmartContract.Manifest.ContractAbi exactly

use crate::error::CoreError;
use crate::smart_contract::i_interoperable::IInteroperable;
use crate::smart_contract::manifest::stack_item_helpers::{
    decode_interoperable_array, expect_struct_items,
};
use crate::smart_contract::manifest::{ContractEventDescriptor, ContractMethodDescriptor};
use neo_vm::StackItem;
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

    /// Gets the method with the specified name
    pub fn get_method(&mut self, name: &str, pcount: i32) -> Option<&ContractMethodDescriptor> {
        // Build dictionary if not already built
        if self.method_dictionary.is_none() {
            let mut dict = HashMap::new();
            for (i, method) in self.methods.iter().enumerate() {
                dict.insert((method.name.clone(), method.parameters.len() as i32), i);
                dict.insert((method.name.clone(), -1), i);
            }
            self.method_dictionary = Some(dict);
        }

        // Look up method
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
    /// Validates the ABI structure.
    pub fn validate(&self) -> Result<(), String> {
        if self.methods.is_empty() {
            return Err("ABI must contain at least one method".to_string());
        }
        Ok(())
    }
}

impl IInteroperable for ContractAbi {
    fn from_stack_item(&mut self, stack_item: StackItem) -> Result<(), CoreError> {
        let items = expect_struct_items(&stack_item, "ContractAbi", 2)?;

        if let Some(methods) = decode_interoperable_array::<ContractMethodDescriptor>(&items[0])? {
            self.methods = methods;
        }

        if let Some(events) = decode_interoperable_array::<ContractEventDescriptor>(&items[1])? {
            self.events = events;
        }
        Ok(())
    }

    fn to_stack_item(&self) -> Result<StackItem, CoreError> {
        let methods = self
            .methods
            .iter()
            .map(|method| method.to_stack_item())
            .collect::<Result<Vec<_>, _>>()?;
        let events = self
            .events
            .iter()
            .map(|event| event.to_stack_item())
            .collect::<Result<Vec<_>, _>>()?;

        Ok(StackItem::from_struct(vec![
            StackItem::from_array(methods),
            StackItem::from_array(events),
        ]))
    }

    fn clone_box(&self) -> Box<dyn IInteroperable> {
        Box::new(self.clone())
    }
}
