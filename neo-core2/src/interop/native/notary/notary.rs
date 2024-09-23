/// Package notary provides an interface to Notary native contract.
/// This contract is a NeoGo extension and is not available on regular Neo
/// networks. To use it, you need to have this extension enabled on the network.

use neo_core2::interop;
use neo_core2::interop::contract;
use neo_core2::interop::neogointernal;

// Hash represents Notary contract hash.
const HASH: &str = "\x3b\xec\x35\x31\x11\x9b\xba\xd7\x6d\xd0\x44\x92\x0b\x0d\xe6\xc3\x19\x4f\xe1\xc1";

// LockDepositUntil represents `lockDepositUntil` method of Notary native contract.
pub fn lock_deposit_until(addr: interop::Hash160, till: i32) -> bool {
    neogointernal::call_with_token(HASH, "lockDepositUntil", contract::States as i32, addr, till).unwrap()
}

// Withdraw represents `withdraw` method of Notary native contract.
pub fn withdraw(from: interop::Hash160, to: interop::Hash160) -> bool {
    neogointernal::call_with_token(HASH, "withdraw", contract::All as i32, from, to).unwrap()
}

// BalanceOf represents `balanceOf` method of Notary native contract.
pub fn balance_of(addr: interop::Hash160) -> i32 {
    neogointernal::call_with_token(HASH, "balanceOf", contract::ReadStates as i32, addr).unwrap()
}

// ExpirationOf represents `expirationOf` method of Notary native contract.
pub fn expiration_of(addr: interop::Hash160) -> i32 {
    neogointernal::call_with_token(HASH, "expirationOf", contract::ReadStates as i32, addr).unwrap()
}

// GetMaxNotValidBeforeDelta represents `getMaxNotValidBeforeDelta` method of Notary native contract.
pub fn get_max_not_valid_before_delta() -> i32 {
    neogointernal::call_with_token(HASH, "getMaxNotValidBeforeDelta", contract::ReadStates as i32).unwrap()
}

// SetMaxNotValidBeforeDelta represents `setMaxNotValidBeforeDelta` method of Notary native contract.
pub fn set_max_not_valid_before_delta(value: i32) {
    neogointernal::call_with_token_no_ret(HASH, "setMaxNotValidBeforeDelta", contract::States as i32, value);
}
