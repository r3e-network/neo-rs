//! NEP-17 transfer history key.
//!
//! Storage key for NEP-17 transfer records.

use super::super::token_transfer_key::TokenTransferKey;
use super::super::tracker_base::TokenTransferKeyView;
use crate::UInt160;
use crate::neo_io::{BinaryWriter, IoResult, MemoryReader, Serializable};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

/// Key for NEP-17 transfer history records.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct Nep17TransferKey(pub TokenTransferKey);

impl Nep17TransferKey {
    /// Creates a new transfer key.
    pub fn new(
        user_script_hash: UInt160,
        timestamp_ms: u64,
        asset_script_hash: UInt160,
        xfer_index: u32,
    ) -> Self {
        Self(TokenTransferKey::new(
            user_script_hash,
            timestamp_ms,
            asset_script_hash,
            xfer_index,
        ))
    }
}

impl PartialOrd for Nep17TransferKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Nep17TransferKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

impl Serializable for Nep17TransferKey {
    fn size(&self) -> usize {
        self.0.size()
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        Serializable::serialize(&self.0, writer)
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        Ok(Self(<TokenTransferKey as Serializable>::deserialize(
            reader,
        )?))
    }
}

impl TokenTransferKeyView for Nep17TransferKey {
    fn user_script_hash(&self) -> &UInt160 {
        &self.0.user_script_hash
    }

    fn timestamp_ms(&self) -> u64 {
        self.0.timestamp_ms
    }

    fn asset_script_hash(&self) -> &UInt160 {
        &self.0.asset_script_hash
    }

    fn block_xfer_notification_index(&self) -> u32 {
        self.0.block_xfer_notification_index
    }
}
