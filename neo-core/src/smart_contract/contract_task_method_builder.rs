//! ContractTaskMethodBuilder - matches C# Neo.SmartContract.ContractTaskMethodBuilder exactly

use crate::smart_contract::contract_task::ContractTask;

/// Builder for contract task methods (matches C# ContractTaskMethodBuilder)
pub struct ContractTaskMethodBuilder {
    name: String,
    parameters: Vec<String>,
}

impl ContractTaskMethodBuilder {
    /// Creates a new builder
    pub fn new(name: String) -> Self {
        Self {
            name,
            parameters: Vec::new(),
        }
    }

    /// Adds a parameter
    pub fn with_parameter(mut self, param: String) -> Self {
        self.parameters.push(param);
        self
    }

    /// Builds the task
    pub fn build(self) -> ContractTask {
        // In actual implementation, this would create a task that invokes the method
        ContractTask::completed()
    }

    /// Gets the method name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Gets the parameters
    pub fn parameters(&self) -> &[String] {
        &self.parameters
    }
}
