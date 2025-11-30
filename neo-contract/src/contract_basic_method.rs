//! ContractBasicMethod - matches C# Neo.SmartContract.ContractBasicMethod exactly.
//!
//! This module provides a guideline for basic methods used in the Neo blockchain,
//! offering a generalized interaction mechanism for smart contract deployment,
//! verification, updates, and destruction.

/// Standard method names and parameter counts for Neo smart contracts.
///
/// This struct provides constants for the basic methods that all Neo smart contracts
/// can implement for standard lifecycle operations.
pub struct ContractBasicMethod;

impl ContractBasicMethod {
    /// The verification method name.
    ///
    /// This must be called when withdrawing tokens from the contract.
    /// If the contract address is included in the transaction signature,
    /// this method verifies the signature.
    pub const VERIFY: &'static str = "verify";

    /// The initialization method name.
    ///
    /// Compiled into the Manifest file if any function uses the initialize statement.
    /// These functions are executed first when loading the contract.
    pub const INITIALIZE: &'static str = "_initialize";

    /// The deployment method name.
    ///
    /// Automatically executed by the ContractManagement contract when a contract
    /// is first deployed or updated.
    pub const DEPLOY: &'static str = "_deploy";

    /// The update method name.
    ///
    /// Requires NefFile or Manifest, or both, and is passed to _deploy.
    /// Should verify the signer's address using SYSCALL Neo.Runtime.CheckWitness.
    pub const UPDATE: &'static str = "update";

    /// The destruction method name.
    ///
    /// Deletes all the storage of the contract.
    /// Should verify the signer's address using SYSCALL Neo.Runtime.CheckWitness.
    /// Any tokens in the contract must be transferred before destruction.
    pub const DESTROY: &'static str = "destroy";

    /// Parameter count for verify method (-1 means arbitrary parameters).
    pub const VERIFY_PARAM_COUNT: i32 = -1;

    /// Parameter count for initialize method.
    pub const INITIALIZE_PARAM_COUNT: i32 = 0;

    /// Parameter count for deploy method.
    pub const DEPLOY_PARAM_COUNT: i32 = 2;

    /// Parameter count for update method.
    pub const UPDATE_PARAM_COUNT: i32 = 3;

    /// Parameter count for destroy method.
    pub const DESTROY_PARAM_COUNT: i32 = 0;

    /// Returns true if the method name is a reserved system method.
    pub fn is_reserved_method(name: &str) -> bool {
        name.starts_with('_')
    }

    /// Returns the parameter count for a given method name.
    pub fn get_param_count(method: &str) -> Option<i32> {
        match method {
            Self::VERIFY => Some(Self::VERIFY_PARAM_COUNT),
            Self::INITIALIZE => Some(Self::INITIALIZE_PARAM_COUNT),
            Self::DEPLOY => Some(Self::DEPLOY_PARAM_COUNT),
            Self::UPDATE => Some(Self::UPDATE_PARAM_COUNT),
            Self::DESTROY => Some(Self::DESTROY_PARAM_COUNT),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_method_names() {
        assert_eq!(ContractBasicMethod::VERIFY, "verify");
        assert_eq!(ContractBasicMethod::INITIALIZE, "_initialize");
        assert_eq!(ContractBasicMethod::DEPLOY, "_deploy");
        assert_eq!(ContractBasicMethod::UPDATE, "update");
        assert_eq!(ContractBasicMethod::DESTROY, "destroy");
    }

    #[test]
    fn test_param_counts() {
        assert_eq!(ContractBasicMethod::VERIFY_PARAM_COUNT, -1);
        assert_eq!(ContractBasicMethod::INITIALIZE_PARAM_COUNT, 0);
        assert_eq!(ContractBasicMethod::DEPLOY_PARAM_COUNT, 2);
        assert_eq!(ContractBasicMethod::UPDATE_PARAM_COUNT, 3);
        assert_eq!(ContractBasicMethod::DESTROY_PARAM_COUNT, 0);
    }

    #[test]
    fn test_is_reserved_method() {
        assert!(ContractBasicMethod::is_reserved_method("_initialize"));
        assert!(ContractBasicMethod::is_reserved_method("_deploy"));
        assert!(ContractBasicMethod::is_reserved_method("_custom"));
        assert!(!ContractBasicMethod::is_reserved_method("verify"));
        assert!(!ContractBasicMethod::is_reserved_method("update"));
        assert!(!ContractBasicMethod::is_reserved_method("destroy"));
    }

    #[test]
    fn test_get_param_count() {
        assert_eq!(ContractBasicMethod::get_param_count("verify"), Some(-1));
        assert_eq!(ContractBasicMethod::get_param_count("_initialize"), Some(0));
        assert_eq!(ContractBasicMethod::get_param_count("_deploy"), Some(2));
        assert_eq!(ContractBasicMethod::get_param_count("update"), Some(3));
        assert_eq!(ContractBasicMethod::get_param_count("destroy"), Some(0));
        assert_eq!(ContractBasicMethod::get_param_count("unknown"), None);
    }
}
