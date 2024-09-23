use std::sync::Arc;
use num_bigint::BigInt;
use crate::util::Uint160;

// Feer is a trait that abstracts the implementation of the fee calculation.
pub trait Feer {
    fn fee_per_byte(&self) -> i64;
    fn get_utility_token_balance(&self, address: &Uint160) -> Arc<BigInt>;
    fn block_height(&self) -> u32;
}
