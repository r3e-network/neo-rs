//! NEP-11 token tracker.

pub mod nep11_balance_key;
pub mod nep11_tracker;
pub mod nep11_transfer_key;

use num_bigint::BigInt;

pub use nep11_balance_key::Nep11BalanceKey;
pub use nep11_tracker::Nep11Tracker;
pub use nep11_transfer_key::Nep11TransferKey;

fn token_id_integer(token: &[u8]) -> BigInt {
    if token.is_empty() {
        BigInt::from(0)
    } else {
        BigInt::from_signed_bytes_le(token)
    }
}

#[cfg(test)]
#[path = "../../../../tests/plugins/tokens_tracker/trackers/nep_11.rs"]
mod tests;
