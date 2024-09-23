/// Package policy provides an interface to PolicyContract native contract.
/// This contract holds various network-wide settings.
mod policy {
    use crate::interop::{self, contract, neogointernal};

    // Hash represents Policy contract hash.
    const HASH: &str = "\x7b\xc6\x81\xc0\xa1\xf7\x1d\x54\x34\x57\xb6\x8b\xba\x8d\x5f\x9f\xdd\x4e\x5e\xcc";

    // GetFeePerByte represents `getFeePerByte` method of Policy native contract.
    pub fn get_fee_per_byte() -> i32 {
        neogointernal::call_with_token(HASH, "getFeePerByte", contract::ReadStates as i32).unwrap()
    }

    // SetFeePerByte represents `setFeePerByte` method of Policy native contract.
    pub fn set_fee_per_byte(value: i32) {
        neogointernal::call_with_token_no_ret(HASH, "setFeePerByte", contract::States as i32, value);
    }

    // GetExecFeeFactor represents `getExecFeeFactor` method of Policy native contract.
    pub fn get_exec_fee_factor() -> i32 {
        neogointernal::call_with_token(HASH, "getExecFeeFactor", contract::ReadStates as i32).unwrap()
    }

    // SetExecFeeFactor represents `setExecFeeFactor` method of Policy native contract.
    pub fn set_exec_fee_factor(value: i32) {
        neogointernal::call_with_token_no_ret(HASH, "setExecFeeFactor", contract::States as i32, value);
    }

    // GetStoragePrice represents `getStoragePrice` method of Policy native contract.
    pub fn get_storage_price() -> i32 {
        neogointernal::call_with_token(HASH, "getStoragePrice", contract::ReadStates as i32).unwrap()
    }

    // SetStoragePrice represents `setStoragePrice` method of Policy native contract.
    pub fn set_storage_price(value: i32) {
        neogointernal::call_with_token_no_ret(HASH, "setStoragePrice", contract::States as i32, value);
    }

    // GetAttributeFee represents `getAttributeFee` method of Policy native contract.
    pub fn get_attribute_fee(t: AttributeType) -> i32 {
        neogointernal::call_with_token(HASH, "getAttributeFee", contract::ReadStates as i32, t).unwrap()
    }

    // SetAttributeFee represents `setAttributeFee` method of Policy native contract.
    pub fn set_attribute_fee(t: AttributeType, value: i32) {
        neogointernal::call_with_token_no_ret(HASH, "setAttributeFee", contract::States as i32, t, value);
    }

    // IsBlocked represents `isBlocked` method of Policy native contract.
    pub fn is_blocked(addr: interop::Hash160) -> bool {
        neogointernal::call_with_token(HASH, "isBlocked", contract::ReadStates as i32, addr).unwrap()
    }

    // BlockAccount represents `blockAccount` method of Policy native contract.
    pub fn block_account(addr: interop::Hash160) -> bool {
        neogointernal::call_with_token(HASH, "blockAccount", contract::States as i32, addr).unwrap()
    }

    // UnblockAccount represents `unblockAccount` method of Policy native contract.
    pub fn unblock_account(addr: interop::Hash160) -> bool {
        neogointernal::call_with_token(HASH, "unblockAccount", contract::States as i32, addr).unwrap()
    }
}
