use super::Notary;
use crate::smart_contract::native::method_macros::neo_native_methods;
use crate::smart_contract::native::NativeMethod;

impl Notary {
    pub(super) fn native_methods() -> Vec<NativeMethod> {
        neo_native_methods![
            safe "balanceOf", fee = 1 << 15, flags = [READ_STATES], params = [Hash160], returns = Integer, names = ["account"];
            safe "expirationOf", fee = 1 << 15, flags = [READ_STATES], params = [Hash160], returns = Integer, names = ["account"];
            safe "getMaxNotValidBeforeDelta", fee = 1 << 15, flags = [READ_STATES], params = [], returns = Integer;
            safe "verify", fee = 1 << 15, flags = [READ_STATES], params = [ByteArray], returns = Boolean, names = ["signature"];
            unsafe "onNEP17Payment", fee = 1 << 15, flags = [STATES], params = [Hash160, Integer, Any], returns = Void, names = ["from", "amount", "data"];
            unsafe "lockDepositUntil", fee = 1 << 15, flags = [STATES], params = [Hash160, Integer], returns = Boolean, names = ["account", "till"];
            unsafe "withdraw", fee = 1 << 15, flags = [ALL], params = [Hash160, Hash160], returns = Boolean, names = ["from", "to"];
            unsafe "setMaxNotValidBeforeDelta", fee = 1 << 15, flags = [STATES], params = [Integer], returns = Void, names = ["value"];
        ]
    }
}
