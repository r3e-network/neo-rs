/// Package gas provides interface to GasToken native contract.
/// It implements regular NEP-17 functions for GAS token.

use neo_core2::interop;
use neo_core2::interop::contract;
use neo_core2::interop::neogointernal;

// Hash represents GAS contract hash.
const HASH: &str = "\u{cf76e28bd0062c4a478ee35561011319f3cfa4d2}";

// Symbol represents `symbol` method of GAS native contract.
pub fn symbol() -> String {
    neogointernal::call_with_token(HASH, "symbol", contract::NoneFlag as i32).unwrap()
}

// Decimals represents `decimals` method of GAS native contract.
pub fn decimals() -> i32 {
    neogointernal::call_with_token(HASH, "decimals", contract::NoneFlag as i32).unwrap()
}

// TotalSupply represents `totalSupply` method of GAS native contract.
pub fn total_supply() -> i32 {
    neogointernal::call_with_token(HASH, "totalSupply", contract::ReadStates as i32).unwrap()
}

// BalanceOf represents `balanceOf` method of GAS native contract.
pub fn balance_of(addr: interop::Hash160) -> i32 {
    neogointernal::call_with_token(HASH, "balanceOf", contract::ReadStates as i32, addr).unwrap()
}

// Transfer represents `transfer` method of GAS native contract.
pub fn transfer(from: interop::Hash160, to: interop::Hash160, amount: i32, data: &dyn std::any::Any) -> bool {
    neogointernal::call_with_token(HASH, "transfer", contract::All as i32, from, to, amount, data).unwrap()
}
