//! ContractBasicMethod - matches C# Neo.SmartContract.ContractBasicMethod exactly

/// This class provides a guideline for basic methods used in the Neo blockchain, offering
/// a generalized interaction mechanism for smart contract deployment, verification, updates, and destruction.
/// (matches C# ContractBasicMethod)
pub struct ContractBasicMethod;

impl ContractBasicMethod {
    /// The verification method. This must be called when withdrawing tokens from the contract.
    /// If the contract address is included in the transaction signature, this method verifies the signature.
    pub const VERIFY: &'static str = "verify";

    /// The initialization method. Compiled into the Manifest file if any function uses the initialize statement.
    /// These functions are executed first when loading the contract.
    pub const INITIALIZE: &'static str = "_initialize";

    /// The deployment method. Automatically executed by the ContractManagement contract when a contract is first deployed or updated.
    pub const DEPLOY: &'static str = "_deploy";

    /// The update method. Requires NefFile or Manifest, or both, and is passed to _deploy.
    /// Should verify the signer's address using SYSCALL Neo.Runtime.CheckWitness.
    pub const UPDATE: &'static str = "update";

    /// The destruction method. Deletes all the storage of the contract.
    /// Should verify the signer's address using SYSCALL Neo.Runtime.CheckWitness.
    /// Any tokens in the contract must be transferred before destruction.
    pub const DESTROY: &'static str = "destroy";

    /// Parameter counts for the methods.
    /// -1 represents the method can take arbitrary parameters.
    pub const VERIFY_P_COUNT: i32 = -1;
    pub const INITIALIZE_P_COUNT: i32 = 0;
    pub const DEPLOY_P_COUNT: i32 = 2;
    pub const UPDATE_P_COUNT: i32 = 3;
    pub const DESTROY_P_COUNT: i32 = 0;
}
