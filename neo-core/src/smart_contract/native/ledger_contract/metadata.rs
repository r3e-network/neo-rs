use super::LedgerContract;
use crate::smart_contract::native::method_macros::neo_native_methods;
use crate::smart_contract::native::NativeMethod;

impl LedgerContract {
    pub(super) fn native_methods() -> Vec<NativeMethod> {
        neo_native_methods![
            safe "currentHash", fee = 1 << 15, flags = [READ_STATES], params = [], returns = Hash256;
            safe "currentIndex", fee = 1 << 15, flags = [READ_STATES], params = [], returns = Integer;
            safe "getBlock", fee = 1 << 15, flags = [READ_STATES], params = [ByteArray], returns = Array, names = ["indexOrHash"];
            safe "getTransaction", fee = 1 << 15, flags = [READ_STATES], params = [Hash256], returns = Array, names = ["hash"];
            safe "getTransactionFromBlock", fee = 1 << 16, flags = [READ_STATES], params = [ByteArray, Integer], returns = Array, names = ["blockIndexOrHash", "txIndex"];
            safe "getTransactionHeight", fee = 1 << 15, flags = [READ_STATES], params = [Hash256], returns = Integer, names = ["hash"];
            safe "getTransactionSigners", fee = 1 << 15, flags = [READ_STATES], params = [Hash256], returns = Array, names = ["hash"];
            safe "getTransactionVMState", fee = 1 << 15, flags = [READ_STATES], params = [Hash256], returns = Integer, names = ["hash"];
        ]
    }
}
