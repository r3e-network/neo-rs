//! NEP-11 transfer history key.
//!
//! Storage key for NEP-11 (NFT) transfer records.

use super::super::token_transfer_key::TokenTransferKey;
use super::token_id_integer;
use neo_core::neo_io::impl_serializable;
use neo_core::UInt160;
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

/// Storage key for NEP-11 transfers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct Nep11TransferKey {
    /// Base transfer key.
    pub base: TokenTransferKey,
    /// Token ID.
    pub token: Vec<u8>,
}

impl Nep11TransferKey {
    /// Creates a new transfer key.
    pub fn new(
        user_script_hash: UInt160,
        timestamp_ms: u64,
        asset_script_hash: UInt160,
        token_id: Vec<u8>,
        xfer_index: u32,
    ) -> Self {
        Self {
            base: TokenTransferKey::new(
                user_script_hash,
                timestamp_ms,
                asset_script_hash,
                xfer_index,
            ),
            token: token_id,
        }
    }

    fn token_integer(&self) -> BigInt {
        token_id_integer(&self.token)
    }
}

impl PartialOrd for Nep11TransferKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Nep11TransferKey {
    fn cmp(&self, other: &Self) -> Ordering {
        let base_cmp = self.base.cmp(&other.base);
        if base_cmp != Ordering::Equal {
            return base_cmp;
        }
        self.token_integer().cmp(&other.token_integer())
    }
}

impl_serializable! {
    struct Nep11TransferKey {
        base: TokenTransferKey,
        // NEP-11 token IDs are bounded to 64 bytes by the standard.
        token: var_bytes { max: 64 },
    }
}

super::super::impl_token_transfer_key_as_ref!(Nep11TransferKey, base);
