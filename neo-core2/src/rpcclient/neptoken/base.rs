/*
Package neptoken contains RPC wrapper for common NEP-11 and NEP-17 methods.

All of these methods are safe, read-only.
*/

use std::str;
use std::sync::Arc;
use num_bigint::BigInt;
use crate::neorpc::result::Invoke;
use crate::rpcclient::unwrap;
use crate::util::Uint160;

const MAX_VALID_DECIMALS: i64 = 77;

// Invoker is used by Base to call various methods.
pub trait Invoker {
    fn call(&self, contract: Uint160, operation: &str, params: Vec<&dyn std::any::Any>) -> Result<Invoke, Box<dyn std::error::Error>>;
}

// Base is a reader interface for common NEP-11 and NEP-17 methods built
// on top of Invoker.
pub struct Base {
    invoker: Arc<dyn Invoker>,
    hash: Uint160,
}

impl Base {
    // New creates an instance of Base for contract with the given hash using the
    // given invoker.
    pub fn new(invoker: Arc<dyn Invoker>, hash: Uint160) -> Self {
        Base { invoker, hash }
    }

    // Decimals implements `decimals` NEP-17 or NEP-11 method and returns the number
    // of decimals used by token. For non-divisible NEP-11 tokens this method always
    // returns zero. Values less than 0 or more than MaxValidDecimals are considered
    // to be invalid (with an appropriate error) even if returned by the contract.
    pub fn decimals(&self) -> Result<i64, Box<dyn std::error::Error>> {
        let r = self.invoker.call(self.hash, "decimals", vec![])?;
        let dec = unwrap::limited_int64(r, 0, MAX_VALID_DECIMALS)?;
        Ok(dec)
    }

    // Symbol implements `symbol` NEP-17 or NEP-11 method and returns a short token
    // identifier (like "NEO" or "GAS").
    pub fn symbol(&self) -> Result<String, Box<dyn std::error::Error>> {
        let r = self.invoker.call(self.hash, "symbol", vec![])?;
        let symbol = unwrap::printable_ascii_string(r)?;
        Ok(symbol)
    }

    // TotalSupply returns the total token supply currently available (the amount
    // of minted tokens).
    pub fn total_supply(&self) -> Result<BigInt, Box<dyn std::error::Error>> {
        let r = self.invoker.call(self.hash, "totalSupply", vec![])?;
        let total_supply = unwrap::big_int(r)?;
        Ok(total_supply)
    }

    // BalanceOf returns the token balance of the given account. For NEP-17 that's
    // the token balance with decimals (1 TOK with 2 decimals will lead to 100
    // returned from this method). For non-divisible NEP-11 that's the number of
    // NFTs owned by the account, for divisible NEP-11 that's the sum of the parts
    // of all NFTs owned by the account.
    pub fn balance_of(&self, account: Uint160) -> Result<BigInt, Box<dyn std::error::Error>> {
        let r = self.invoker.call(self.hash, "balanceOf", vec![&account])?;
        let balance = unwrap::big_int(r)?;
        Ok(balance)
    }
}
