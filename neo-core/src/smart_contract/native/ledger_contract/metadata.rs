use super::LedgerContract;
use crate::smart_contract::native::method_macros::{
    neo_native_method_dispatch, neo_native_method_metadata,
};
use crate::smart_contract::native::NativeMethod;

macro_rules! ledger_method_table {
    ($callback:ident; $($args:tt)*) => {
        $callback! {
            $($args)*
            ;
            {
                safe "currentHash", fee = 1 << 15, flags = [READ_STATES], params = [], returns = Hash256 => engine invoke_current_hash;
                safe "currentIndex", fee = 1 << 15, flags = [READ_STATES], params = [], returns = Integer => engine invoke_current_index;
                safe "getBlock", fee = 1 << 15, flags = [READ_STATES], params = [ByteArray], returns = Array, names = ["indexOrHash"] => engine invoke_get_block;
                safe "getTransaction", fee = 1 << 15, flags = [READ_STATES], params = [Hash256], returns = Array, names = ["hash"] => engine invoke_get_transaction;
                safe "getTransactionFromBlock", fee = 1 << 16, flags = [READ_STATES], params = [ByteArray, Integer], returns = Array, names = ["blockIndexOrHash", "txIndex"] => engine invoke_get_transaction_from_block;
                safe "getTransactionHeight", fee = 1 << 15, flags = [READ_STATES], params = [Hash256], returns = Integer, names = ["hash"] => engine invoke_get_transaction_height;
                safe "getTransactionSigners", fee = 1 << 15, flags = [READ_STATES], params = [Hash256], returns = Array, names = ["hash"] => engine invoke_get_transaction_signers;
                safe "getTransactionVMState", fee = 1 << 15, flags = [READ_STATES], params = [Hash256], returns = Integer, names = ["hash"] => engine invoke_get_transaction_vm_state;
            }
        }
    };
}

impl LedgerContract {
    pub(super) fn native_methods() -> Vec<NativeMethod> {
        ledger_method_table!(neo_native_method_metadata;)
    }

    pub(super) fn dispatch_method(
        &self,
        engine: &mut crate::smart_contract::ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> crate::error::CoreResult<Vec<u8>> {
        ledger_method_table!(
            neo_native_method_dispatch;
            self,
            engine,
            method,
            args,
            aliases = [],
            unknown = |method| crate::error::CoreError::native_contract(format!("Method {} not found", method))
        )
    }
}
