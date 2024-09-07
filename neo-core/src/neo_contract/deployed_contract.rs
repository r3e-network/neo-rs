use crate::smart_contract::manifest::ContractState;
use crate::smart_contract::Contract;
use crate::types::UInt160;
use crate::smart_contract::ContractMethodDescriptor;
use crate::smart_contract::ContractBasicMethod;

/// Represents a deployed contract that can be invoked.
pub struct DeployedContract {
    pub script_hash: UInt160,
    pub parameter_list: Vec<ContractParameterType>,
}

impl DeployedContract {
    /// Initializes a new instance of the DeployedContract struct with the specified ContractState.
    ///
    /// # Arguments
    ///
    /// * `contract` - The ContractState corresponding to the contract.
    ///
    /// # Panics
    ///
    /// Panics if the contract is null or if the smart contract doesn't have a verify method.
    pub fn new(contract: &ContractState) -> Self {
        if contract.is_none() {
            panic!("Contract is null");
        }

        let script_hash = contract.hash.clone();
        let descriptor = contract.manifest.abi.get_method(
            ContractBasicMethod::Verify,
            ContractBasicMethod::VerifyPCount,
        );

        if descriptor.is_none() {
            panic!("The smart contract doesn't have a verify method.");
        }

        let parameter_list = descriptor.unwrap().parameters.iter()
            .map(|p| p.parameter_type)
            .collect();

        DeployedContract {
            script_hash,
            parameter_list,
        }
    }
}

impl Contract for DeployedContract {
    fn script_hash(&self) -> &UInt160 {
        &self.script_hash
    }
}
