/*
Package management provides an interface to ContractManagement native contract.
It allows to get/deploy/update contracts as well as get/set deployment fee.
*/
mod management {
    use crate::interop::{self, contract, iterator, neogointernal};
    use crate::interop::interop::Hash160;

    // Hash represents Management contract hash.
    const HASH: &str = "\xfd\xa3\xfa\x43\x46\xea\x53\x2a\x25\x8f\xc4\x97\xdd\xad\xdb\x64\x37\xc9\xfd\xff";

    // IDHash is an ID/Hash pair returned by the iterator from the GetContractHashes method.
    pub struct IDHash {
        // ID is a 32-bit number, but it's represented in big endian form
        // natively, because that's the key scheme used by ContractManagement.
        pub id: Vec<u8>,
        pub hash: Hash160,
    }

    // Deploy represents `deploy` method of Management native contract.
    pub fn deploy(script: &[u8], manifest: &[u8]) -> Contract {
        neogointernal::call_with_token(HASH, "deploy", contract::ALL as i32, script, manifest)
    }

    // DeployWithData represents `deploy` method of Management native contract.
    pub fn deploy_with_data(script: &[u8], manifest: &[u8], data: &dyn std::any::Any) -> Contract {
        neogointernal::call_with_token(HASH, "deploy", contract::ALL as i32, script, manifest, data)
    }

    // Destroy represents `destroy` method of Management native contract.
    pub fn destroy() {
        neogointernal::call_with_token_no_ret(HASH, "destroy", (contract::STATES | contract::ALLOW_NOTIFY) as i32);
    }

    // GetContract represents `getContract` method of Management native contract.
    pub fn get_contract(addr: Hash160) -> Contract {
        neogointernal::call_with_token(HASH, "getContract", contract::READ_STATES as i32, addr)
    }

    // GetContractByID represents `getContractById` method of the Management native contract.
    pub fn get_contract_by_id(id: i32) -> Contract {
        neogointernal::call_with_token(HASH, "getContractById", contract::READ_STATES as i32, id)
    }

    // GetContractHashes represents `getContractHashes` method of the Management
    // native contract. It returns an Iterator over the list of non-native contract
    // hashes. Each iterator value can be cast to IDHash. Use [iterator] interop
    // package to work with the returned Iterator.
    pub fn get_contract_hashes() -> iterator::Iterator {
        neogointernal::call_with_token(HASH, "getContractHashes", contract::READ_STATES as i32)
    }

    // GetMinimumDeploymentFee represents `getMinimumDeploymentFee` method of Management native contract.
    pub fn get_minimum_deployment_fee() -> i32 {
        neogointernal::call_with_token(HASH, "getMinimumDeploymentFee", contract::READ_STATES as i32)
    }

    // HasMethod represents `hasMethod` method of Management native contract. It allows to check
    // if the "hash" contract has a method named "method" with parameters number equal to "pcount".
    pub fn has_method(hash: Hash160, method: &str, pcount: i32) -> bool {
        neogointernal::call_with_token(HASH, "hasMethod", contract::READ_STATES as i32, hash, method, pcount)
    }

    // SetMinimumDeploymentFee represents `setMinimumDeploymentFee` method of Management native contract.
    pub fn set_minimum_deployment_fee(value: i32) {
        neogointernal::call_with_token_no_ret(HASH, "setMinimumDeploymentFee", contract::STATES as i32, value);
    }

    // Update represents `update` method of Management native contract.
    pub fn update(script: &[u8], manifest: &[u8]) {
        neogointernal::call_with_token_no_ret(HASH, "update", contract::ALL as i32, script, manifest);
    }

    // UpdateWithData represents `update` method of Management native contract.
    pub fn update_with_data(script: &[u8], manifest: &[u8], data: &dyn std::any::Any) {
        neogointernal::call_with_token_no_ret(HASH, "update", contract::ALL as i32, script, manifest, data);
    }
}
