//! ContractMethodAttribute - matches C# Neo.SmartContract.Native.ContractMethodAttribute exactly

use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::contract_parameter_type::ContractParameterType;

/// Attribute to mark contract methods (matches C# ContractMethodAttribute)
#[derive(Clone, Debug)]
pub struct ContractMethodAttribute {
    /// The order/index of the method
    pub order: u32,
    
    /// The name of the method
    pub name: String,
    
    /// The required call flags
    pub required_call_flags: CallFlags,
    
    /// Parameter types
    pub parameter_types: Vec<ContractParameterType>,
    
    /// Return type
    pub return_type: ContractParameterType,
    
    /// Whether the method is safe
    pub safe: bool,
}

impl ContractMethodAttribute {
    /// Creates a new contract method attribute
    pub fn new(order: u32, name: String) -> Self {
        Self {
            order,
            name,
            required_call_flags: CallFlags::None,
            parameter_types: Vec::new(),
            return_type: ContractParameterType::Void,
            safe: false,
        }
    }
    
    /// Sets the required call flags
    pub fn with_call_flags(mut self, flags: CallFlags) -> Self {
        self.required_call_flags = flags;
        self
    }
    
    /// Adds a parameter type
    pub fn add_parameter(mut self, param_type: ContractParameterType) -> Self {
        self.parameter_types.push(param_type);
        self
    }
    
    /// Sets the return type
    pub fn with_return_type(mut self, return_type: ContractParameterType) -> Self {
        self.return_type = return_type;
        self
    }
    
    /// Sets whether the method is safe
    pub fn with_safe(mut self, safe: bool) -> Self {
        self.safe = safe;
        self
    }
    
    /// Creates the method descriptor
    pub fn to_descriptor(&self, offset: i32) -> crate::smart_contract::manifest::ContractMethodDescriptor {
        let params = self.parameter_types.iter().enumerate()
            .map(|(i, &param_type)| {
                crate::smart_contract::manifest::ContractParameterDefinition::new(
                    format!("arg{}", i),
                    param_type,
                ).unwrap()
            })
            .collect();
        
        crate::smart_contract::manifest::ContractMethodDescriptor::new(
            self.name.clone(),
            params,
            self.return_type,
            offset,
            self.safe,
        ).unwrap()
    }
}
