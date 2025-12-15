//! Base key used for NEP transfer history.
//!
//! Common transfer key structure used by both NEP-11 and NEP-17.

use super::tracker_base::TokenTransferKeyView;
use crate::neo_io::{BinaryWriter, IoResult, MemoryReader, Serializable};
use crate::UInt160;
use serde::{Deserialize, Serialize};

/// Key for transfer history records.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct TokenTransferKey {
    /// User's script hash.
    pub user_script_hash: UInt160,
    /// Timestamp in milliseconds.
    pub timestamp_ms: u64,
    /// Asset's script hash.
    pub asset_script_hash: UInt160,
    /// Notification index within the block.
    pub block_xfer_notification_index: u32,
}

impl TokenTransferKey {
    /// Creates a new transfer key.
    pub fn new(
        user_script_hash: UInt160,
        timestamp_ms: u64,
        asset_script_hash: UInt160,
        xfer_index: u32,
    ) -> Self {
        Self {
            user_script_hash,
            timestamp_ms,
            asset_script_hash,
            block_xfer_notification_index: xfer_index,
        }
    }
}

crate::impl_ord_by_fields!(
    TokenTransferKey,
    user_script_hash,
    timestamp_ms,
    asset_script_hash,
    block_xfer_notification_index
);

impl Serializable for TokenTransferKey {
    fn size(&self) -> usize {
        20 + 8 + 20 + 4
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_serializable(&self.user_script_hash)?;
        writer.write_u64(self.timestamp_ms.to_be())?;
        writer.write_serializable(&self.asset_script_hash)?;
        writer.write_u32(self.block_xfer_notification_index)?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let user_script_hash = <UInt160 as Serializable>::deserialize(reader)?;
        let timestamp_ms = u64::from_be(reader.read_u64()?);
        let asset_script_hash = <UInt160 as Serializable>::deserialize(reader)?;
        let block_xfer_notification_index = reader.read_u32()?;
        Ok(Self {
            user_script_hash,
            timestamp_ms,
            asset_script_hash,
            block_xfer_notification_index,
        })
    }
}

impl TokenTransferKeyView for TokenTransferKey {
    fn user_script_hash(&self) -> &UInt160 {
        &self.user_script_hash
    }

    fn timestamp_ms(&self) -> u64 {
        self.timestamp_ms
    }

    fn asset_script_hash(&self) -> &UInt160 {
        &self.asset_script_hash
    }

    fn block_xfer_notification_index(&self) -> u32 {
        self.block_xfer_notification_index
    }
}
