//! `ChangeView` message - request to change the current view.

use crate::{ChangeViewReason, ConsensusMessageType, ConsensusResult};
use neo_io::serializable::helper::SerializeHelper;
use neo_io::{BinaryWriter, MemoryReader};
use neo_primitives::UInt256;
use serde::{Deserialize, Serialize};

/// `ChangeView` message sent when a validator wants to change the view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeViewMessage {
    /// Block index
    pub block_index: u32,
    /// Current view number
    pub view_number: u8,
    /// Validator index
    pub validator_index: u8,
    /// Timestamp of the request
    pub timestamp: u64,
    /// Reason for the view change
    pub reason: ChangeViewReason,
    /// Transaction hashes the sender rejected. C# v3.10.0 carries this on the
    /// wire ONLY for the `TxRejectedByPolicy`/`TxInvalid` reasons; empty for all
    /// others. Feeds the primary's `InvalidTransactions` F-skip.
    #[serde(default)]
    pub rejected_hashes: Vec<UInt256>,
}

impl ChangeViewMessage {
    /// Creates a new `ChangeView` message
    #[must_use]
    pub const fn new(
        block_index: u32,
        view_number: u8,
        validator_index: u8,
        timestamp: u64,
        reason: ChangeViewReason,
    ) -> Self {
        Self {
            block_index,
            view_number,
            validator_index,
            timestamp,
            reason,
            rejected_hashes: Vec::new(),
        }
    }

    /// Attaches the rejected transaction hashes (only meaningful for the
    /// `TxRejectedByPolicy`/`TxInvalid` reasons, which serialize them on the wire).
    #[must_use]
    pub fn with_rejected_hashes(mut self, rejected_hashes: Vec<UInt256>) -> Self {
        self.rejected_hashes = rejected_hashes;
        self
    }

    /// C# v3.10.0 `ChangeView` serializes `RejectedHashes` ONLY for these reasons.
    const fn serializes_rejected_hashes(&self) -> bool {
        matches!(
            self.reason,
            ChangeViewReason::TxRejectedByPolicy | ChangeViewReason::TxInvalid
        )
    }

    /// `NewViewNumber` is always `ViewNumber + 1` (matches C# `DBFTPlugin`).
    pub fn new_view_number(&self) -> ConsensusResult<u8> {
        self.view_number
            .checked_add(1)
            .ok_or_else(|| crate::ConsensusError::invalid_proposal("View number overflow"))
    }

    /// Returns the message type
    #[must_use]
    pub const fn message_type(&self) -> ConsensusMessageType {
        ConsensusMessageType::ChangeView
    }

    /// Serializes the message to bytes.
    ///
    /// Neo N3 `DBFTPlugin` format: `timestamp (8) + reason (1)`, then — for the
    /// `TxRejectedByPolicy`/`TxInvalid` reasons only — the `RejectedHashes`
    /// `UInt256[]` (var-int count + 32 bytes each), matching C# v3.10.0
    /// `ChangeView.Serialize` (`writer.Write(RejectedHashes)`).
    #[must_use]
    pub fn serialize(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&self.timestamp.to_le_bytes());
        data.push(self.reason.to_byte());
        if self.serializes_rejected_hashes() {
            let mut writer = BinaryWriter::new();
            let _ = SerializeHelper::serialize_array(&self.rejected_hashes, &mut writer);
            data.extend_from_slice(&writer.into_bytes());
        }
        data
    }

    /// Deserializes a `ChangeView` message from bytes (body only, header passed in).
    ///
    /// Mirrors C# v3.10.0 `ChangeView.Deserialize`: reads `timestamp (8) +
    /// reason (1)`, then — for the `TxRejectedByPolicy`/`TxInvalid` reasons only —
    /// the `RejectedHashes` array (`ReadSerializableArray<UInt256>(ushort.MaxValue)`).
    pub fn deserialize(
        data: &[u8],
        block_index: u32,
        view_number: u8,
        validator_index: u8,
    ) -> ConsensusResult<Self> {
        if data.len() < 9 {
            return Err(crate::ConsensusError::invalid_proposal(
                "ChangeView message too short",
            ));
        }

        let timestamp = u64::from_le_bytes(data[0..8].try_into().unwrap_or([0u8; 8]));
        let reason = ChangeViewReason::from_byte(data[8]).unwrap_or(ChangeViewReason::Timeout);

        let mut message = Self {
            block_index,
            view_number,
            validator_index,
            timestamp,
            reason,
            rejected_hashes: Vec::new(),
        };

        if message.serializes_rejected_hashes() {
            let mut reader = MemoryReader::new(&data[9..]);
            message.rejected_hashes =
                SerializeHelper::deserialize_array::<UInt256>(&mut reader, u16::MAX as usize)
                    .map_err(|_| {
                        crate::ConsensusError::invalid_proposal("ChangeView rejected hashes")
                    })?;
        }

        Ok(message)
    }

    /// Validates the message
    pub fn validate(&self) -> ConsensusResult<()> {
        // Ensure NewViewNumber is representable and strictly larger than ViewNumber.
        let new_view = self.new_view_number()?;
        if new_view <= self.view_number {
            return Err(crate::ConsensusError::invalid_proposal(
                "Invalid ChangeView new view number",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/messages/change_view.rs"]
mod tests;
