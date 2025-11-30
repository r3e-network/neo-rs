//! DeployedContract - matches C# Neo.SmartContract.DeployedContract exactly

use crate::smart_contract::contract_state::{ContractState, NefFile};
use crate::smart_contract::manifest::ContractManifest;
use crate::UInt160;

/// Represents a deployed contract (matches C# DeployedContract)
#[derive(Clone, Debug)]
pub struct DeployedContract {
    /// The contract state
    pub state: ContractState,
}

impl DeployedContract {
    /// Creates a new deployed contract
    pub fn new(state: ContractState) -> Self {
        Self { state }
    }

    /// Gets the contract ID
    pub fn id(&self) -> i32 {
        self.state.id
    }

    /// Gets the contract hash
    pub fn hash(&self) -> UInt160 {
        self.state.hash
    }

    /// Gets the NEF file
    pub fn nef(&self) -> &NefFile {
        &self.state.nef
    }

    /// Gets the manifest
    pub fn manifest(&self) -> &ContractManifest {
        &self.state.manifest
    }

    /// Gets the update counter
    pub fn update_counter(&self) -> u16 {
        self.state.update_counter
    }
}
