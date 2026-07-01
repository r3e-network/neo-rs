//! # neo-rpc::plugins::tokens_tracker::trackers::nep_11
//!
//! NEP-11 token tracking helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `nep11_balance_key`: NEP-11 balance key records.
//! - `nep11_tracker`: NEP-11 tracker implementation.
//! - `nep11_transfer_key`: NEP-11 transfer key records.
//! - `tests`: Module-local tests and regression coverage.

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
