//! ContractEventDescriptor - matches C# Neo.SmartContract.Manifest.ContractEventDescriptor exactly

use crate::error::CoreError;
use crate::smart_contract::i_interoperable::IInteroperable;
use crate::smart_contract::manifest::ContractParameterDefinition;
use crate::smart_contract::manifest::stack_item_helpers::{
    decode_interoperable_array, expect_struct_items,
};
use crate::smart_contract::stack_item_extract::extract_string;
use neo_vm::StackItem;
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
    pub fn new(name: String, parameters: Vec<ContractParameterDefinition>) -> Result<Self, String> {
        if name.is_empty() {
            return Err("Event name cannot be empty".to_string());
        }

        // Check for duplicate parameter names
        let mut names = std::collections::HashSet::new();
        for param in &parameters {
            if !names.insert(&param.name) {
                return Err(format!("Duplicate parameter name: {}", param.name));
            }
        }

        Ok(Self { name, parameters })
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
}

impl IInteroperable for ContractEventDescriptor {
    fn from_stack_item(&mut self, stack_item: StackItem) -> Result<(), CoreError> {
        let items = expect_struct_items(&stack_item, "ContractEventDescriptor", 2)?;

        if let Some(name) = extract_string(&items[0]) {
            self.name = name;
        }

        if let Some(parameters) =
            decode_interoperable_array::<ContractParameterDefinition>(&items[1])?
        {
            self.parameters = parameters;
        }
        Ok(())
    }

    fn to_stack_item(&self) -> Result<StackItem, CoreError> {
        let params = self
            .parameters
            .iter()
            .map(|p| p.to_stack_item())
            .collect::<Result<Vec<_>, _>>()?;
        Ok(StackItem::from_struct(vec![
            StackItem::from_byte_string(self.name.as_bytes()),
            StackItem::from_array(params),
        ]))
    }

    fn clone_box(&self) -> Box<dyn IInteroperable> {
        Box::new(self.clone())
    }
}
