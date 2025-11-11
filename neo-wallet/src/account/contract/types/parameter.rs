use alloc::string::String;

use super::ContractParameterType;

#[derive(Clone, Debug)]
pub struct ContractParameter {
    name: String,
    parameter_type: ContractParameterType,
}

impl ContractParameter {
    pub fn new(name: impl Into<String>, parameter_type: ContractParameterType) -> Self {
        Self {
            name: name.into(),
            parameter_type,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn parameter_type(&self) -> ContractParameterType {
        self.parameter_type
    }
}
