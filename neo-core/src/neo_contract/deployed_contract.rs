use crate::neo_contract::contract::Contract;
use crate::neo_contract::contract_basic_method::ContractBasicMethod;
use crate::neo_contract::contract_parameter_type::ContractParameterType;
use crate::neo_contract::contract_state::ContractState;
use neo_type::H160;

/// Represents a deployed contract that can be invoked.
pub struct DeployedContract {
    pub script_hash: H160,
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
    pub fn new(contract: &mut ContractState) -> Self {
        if contract.is_none() {
            panic!("Contract is null");
        }

        let script_hash = contract.hash.clone();
        let descriptor = contract.manifest.abi.get_method(
            ContractBasicMethod::VERIFY,
            ContractBasicMethod::VERIFY_P_COUNT,
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
    fn script(&self) -> &Vec<u8> {
        todo!()
    }

    fn parameter_list(&self) -> &Vec<ContractParameterType> {
        todo!()
    }

    fn script_hash(&self) -> &H160 {
        &self.script_hash
    }
}
