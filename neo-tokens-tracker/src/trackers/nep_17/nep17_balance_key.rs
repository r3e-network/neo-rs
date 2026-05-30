//! NEP-17 balance key.
//!
//! Storage key for NEP-17 token balances.

use neo_core::neo_io::impl_serializable;
use neo_core::UInt160;
use serde::{Deserialize, Serialize};

/// Key for NEP-17 balance records.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct Nep17BalanceKey {
    /// User's script hash.
    pub user_script_hash: UInt160,
    /// Token contract's script hash.
    pub asset_script_hash: UInt160,
}

impl Nep17BalanceKey {
    /// Creates a new balance key.
    pub fn new(user_script_hash: UInt160, asset_script_hash: UInt160) -> Self {
        Self {
            user_script_hash,
            asset_script_hash,
        }
    }
}

neo_core::impl_ord_by_fields!(Nep17BalanceKey, user_script_hash, asset_script_hash);

impl_serializable! {
    struct Nep17BalanceKey {
        user_script_hash: UInt160,
        asset_script_hash: UInt160,
    }
}
