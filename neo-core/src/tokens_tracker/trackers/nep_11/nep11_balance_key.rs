//! NEP-11 balance key.
//!
//! Storage key for NEP-11 (NFT) balances.

use super::token_id_integer;
use crate::neo_io::impl_serializable;
use crate::UInt160;
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

/// Storage key for NEP-11 balances: `[UserScriptHash, AssetScriptHash, TokenId]`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct Nep11BalanceKey {
    /// User's script hash.
    pub user_script_hash: UInt160,
    /// NFT contract's script hash.
    pub asset_script_hash: UInt160,
    /// Token ID.
    pub token: Vec<u8>,
}

impl Nep11BalanceKey {
    /// Creates a new balance key.
    pub fn new(user_script_hash: UInt160, asset_script_hash: UInt160, token_id: Vec<u8>) -> Self {
        Self {
            user_script_hash,
            asset_script_hash,
            token: token_id,
        }
    }

    fn token_integer(&self) -> BigInt {
        token_id_integer(&self.token)
    }
}

impl PartialOrd for Nep11BalanceKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Nep11BalanceKey {
    fn cmp(&self, other: &Self) -> Ordering {
        let user_cmp = self.user_script_hash.cmp(&other.user_script_hash);
        if user_cmp != Ordering::Equal {
            return user_cmp;
        }
        let asset_cmp = self.asset_script_hash.cmp(&other.asset_script_hash);
        if asset_cmp != Ordering::Equal {
            return asset_cmp;
        }
        self.token_integer().cmp(&other.token_integer())
    }
}

impl_serializable! {
    struct Nep11BalanceKey {
        user_script_hash: UInt160,
        asset_script_hash: UInt160,
        token: var_bytes { max: usize::MAX },
    }
}
