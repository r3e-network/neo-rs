// Copyright (C) 2015-2024 The Neo Project.
//
// contract_basic_method.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo::prelude::*;

/// This struct provides a guideline for basic methods used in the Neo blockchain, offering
/// a generalized interaction mechanism for smart contract deployment, verification, updates, and destruction.
pub struct ContractBasicMethod;

impl ContractBasicMethod {
    /// The verification method. This must be called when withdrawing tokens from the contract.
    /// If the contract address is included in the transaction signature, this method verifies the signature.
    /// Example:
    /// ```
    /// #[no_mangle]
    /// pub extern "C" fn verify() -> bool {
    ///     runtime::check_witness(&OWNER)
    /// }
    /// ```
    /// 
    /// Manifest:
    /// ```json
    /// {
    ///   "name": "verify",
    ///   "safe": false,
    ///   "parameters": [],
    ///   "returntype": "Boolean"
    /// }
    /// ```
    pub const VERIFY: &'static str = "verify";

    /// The initialization method. Compiled into the `Manifest` file if any function uses the initialize statement.
    /// These functions are executed first when loading the contract.
    /// Example:
    /// ```
    /// static OWNER: Lazy<Address> = Lazy::new(|| {
    ///     Address::from_str("NdUL5oDPD159KeFpD5A9zw5xNF1xLX6nLT").unwrap()
    /// });
    /// ```
    pub const INITIALIZE: &'static str = "_initialize";

    /// The deployment method. Automatically executed by the ContractManagement contract when a contract is first deployed or updated.
    /// ```json
    /// {
    ///     "name": "_deploy",
    ///     "safe": false,
    ///     "parameters": [
    ///     {
    ///         "name": "data",
    ///         "type": "Any"
    ///     },
    ///     {
    ///         "name": "update",
    ///         "type": "Boolean"
    ///     }
    ///     ],
    ///     "returntype": "Void"
    /// }
    /// ```
    pub const DEPLOY: &'static str = "_deploy";

    /// The update method. Requires `NefFile` or `Manifest`, or both, and is passed to _deploy.
    /// Should verify the signer's address using SYSCALL `Neo.Runtime.CheckWitness`.
    /// ```json
    /// {
    ///   "name": "update",
    ///   "safe": false,
    ///   "parameters": [
    ///     {
    ///       "name": "nefFile",
    ///       "type": "ByteArray"
    ///     },
    ///     {
    ///       "name": "manifest",
    ///       "type": "ByteArray"
    ///     },
    ///     {
    ///       "name": "data",
    ///       "type": "Any"
    ///     }
    ///   ],
    ///   "returntype": "Void"
    /// }
    /// ```
    pub const UPDATE: &'static str = "update";

    /// The destruction method. Deletes all the storage of the contract.
    /// Should verify the signer's address using SYSCALL `Neo.Runtime.CheckWitness`.
    /// Any tokens in the contract must be transferred before destruction.
    /// ```json
    /// {
    ///   "name": "destroy",
    ///   "safe": false,
    ///   "parameters": [],
    ///   "returntype": "Void"
    /// }
    /// ```
    pub const DESTROY: &'static str = "destroy";

    /// Parameter counts for the methods.
    /// -1 represents the method can take arbitrary parameters.
    pub const VERIFY_P_COUNT: i32 = -1;
    pub const INITIALIZE_P_COUNT: i32 = 0;
    pub const DEPLOY_P_COUNT: i32 = 2;
    pub const UPDATE_P_COUNT: i32 = 3;
    pub const DESTROY_P_COUNT: i32 = 0;
}
