//! ContractEventAttribute - matches C# Neo.SmartContract.Native.ContractEventAttribute exactly

use crate::smart_contract::ContractParameterType;

/// Attribute to mark contract events (matches C# ContractEventAttribute)
#[derive(Clone, Debug)]
pub struct ContractEventAttribute {
    /// The order/index of the event
    pub order: u32,
    
    /// The name of the event
    pub name: String,
    
    /// Parameter names and types
    pub parameters: Vec<(String, ContractParameterType)>,
}

impl ContractEventAttribute {
    /// Creates a new contract event attribute
    pub fn new(order: u32, name: String) -> Self {
        Self {
            order,
            name,
            parameters: Vec::new(),
        }
    }
    
    /// Adds a parameter to the event
    pub fn add_parameter(mut self, name: String, param_type: ContractParameterType) -> Self {
        self.parameters.push((name, param_type));
        self
    }
    
    /// Creates the event descriptor
    pub fn to_descriptor(&self) -> crate::smart_contract::manifest::ContractEventDescriptor {
        let params = self.parameters.iter()
            .map(|(name, param_type)| {
                crate::smart_contract::manifest::ContractParameterDefinition::new(
                    name.clone(),
                    *param_type,
                ).unwrap()
            })
            .collect();
        
        crate::smart_contract::manifest::ContractEventDescriptor::new(
            self.name.clone(),
            params,
        ).unwrap()
    }
}