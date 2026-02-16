//! NEP-11 transfer history key.
//!
//! Storage key for NEP-11 (NFT) transfer records.

use super::super::super::extensions::bytes_var_size;
use super::super::token_transfer_key::TokenTransferKey;
use super::super::tracker_base::TokenTransferKeyView;
use crate::UInt160;
use crate::neo_io::{BinaryWriter, IoResult, MemoryReader, Serializable};
use neo_vm::stack_item::ByteString;
use num_bigint::BigInt;
use num_traits::Zero;
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
        ByteString::new(self.token.clone())
            .to_integer()
            .unwrap_or_else(|_| BigInt::zero())
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

impl Serializable for Nep11TransferKey {
    fn size(&self) -> usize {
        self.base.size() + bytes_var_size(self.token.len())
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        Serializable::serialize(&self.base, writer)?;
        writer.write_var_bytes(&self.token)?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let base = <TokenTransferKey as Serializable>::deserialize(reader)?;
        let token = reader.read_var_bytes(usize::MAX)?;
        Ok(Self { base, token })
    }
}

impl TokenTransferKeyView for Nep11TransferKey {
    fn user_script_hash(&self) -> &UInt160 {
        &self.base.user_script_hash
    }

    fn timestamp_ms(&self) -> u64 {
        self.base.timestamp_ms
    }

    fn asset_script_hash(&self) -> &UInt160 {
        &self.base.asset_script_hash
    }

    fn block_xfer_notification_index(&self) -> u32 {
        self.base.block_xfer_notification_index
    }
}
