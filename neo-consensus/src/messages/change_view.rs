//! `ChangeView` message - request to change the current view.

use crate::{ChangeViewReason, ConsensusMessageType, ConsensusResult};
use neo_io::MemoryReader;
use neo_io::serializable::helper::SerializeHelper;
use neo_primitives::UInt256;
use serde::{Deserialize, Serialize};

use super::wire::{append_uint256_array, uint256_array_encoded_len};

/// `ChangeView` message sent when a validator wants to change the view.
///
/// Wire format matches C# `DBFTPlugin` `ChangeView` (v3.10.0,
/// `DBFTPlugin/Messages/ChangeView.cs`): after the common consensus-message
/// header, the body is `Timestamp (u64 LE) + Reason (u8)`, followed —
/// CONDITIONALLY — by `RejectedHashes` (a `UInt256[]`: var-int count then 32 raw
/// bytes per hash) ONLY when `Reason` is `TxRejectedByPolicy` (0x3) or
/// `TxInvalid` (0x4). For every other reason there is NO trailing array.
///
/// The `RejectedHashes` array is the SIGNED body for those two reasons, so it
/// MUST be reproduced byte-for-byte or signature verification diverges from C#
/// peers in both directions. (A prior revision incorrectly dropped this field on
/// the belief that "no C# dBFT version carries it" — v3.10.0 does.) The array
/// uses the same var-int-count + raw-32-bytes-each encoding as
/// `PrepareRequest.transaction_hashes`, capped at `ushort.MaxValue` (65535) to
/// match `ReadSerializableArray<UInt256>(ushort.MaxValue)`.
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
    /// Rejected transaction hashes. Serialized ONLY when `reason` is
    /// `TxRejectedByPolicy`/`TxInvalid` (C# `ChangeView.RejectedHashes`); empty
    /// and never written for all other reasons.
    pub rejected_hashes: Vec<UInt256>,
}

impl ChangeViewMessage {
    /// Creates a new `ChangeView` message.
    ///
    /// `rejected_hashes` is only serialized when `reason` is
    /// `TxRejectedByPolicy`/`TxInvalid`; pass `Vec::new()` for all other reasons.
    #[must_use]
    pub fn new(
        block_index: u32,
        view_number: u8,
        validator_index: u8,
        timestamp: u64,
        reason: ChangeViewReason,
        rejected_hashes: Vec<UInt256>,
    ) -> Self {
        Self {
            block_index,
            view_number,
            validator_index,
            timestamp,
            reason,
            rejected_hashes,
        }
    }

    /// Returns `true` when this reason carries a trailing `RejectedHashes`
    /// `UInt256[]` in the signed body (C# `ChangeView` reasons 0x3/0x4).
    const fn reason_carries_rejected_hashes(reason: ChangeViewReason) -> bool {
        matches!(
            reason,
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

    /// Serializes the message body to bytes, matching C# `DBFTPlugin`
    /// `ChangeView.Serialize` (v3.10.0): `Timestamp (u64 LE) + Reason (u8)`,
    /// followed by `RejectedHashes` (a `UInt256[]`: var-int count then 32 raw
    /// bytes each) ONLY when `reason` is `TxRejectedByPolicy`/`TxInvalid`.
    ///
    /// The `UInt256[]` uses the SAME `write_serializable_vec` encoding as
    /// `PrepareRequest.transaction_hashes`, so the byte layout matches C#
    /// `writer.Write(RejectedHashes)` exactly.
    #[must_use]
    pub fn serialize(&self) -> Vec<u8> {
        let rejected_hashes_len = if Self::reason_carries_rejected_hashes(self.reason) {
            uint256_array_encoded_len(&self.rejected_hashes)
        } else {
            0
        };
        let mut bytes = Vec::with_capacity(8 + 1 + rejected_hashes_len);
        bytes.extend_from_slice(&self.timestamp.to_le_bytes());
        bytes.push(self.reason.to_byte());
        if Self::reason_carries_rejected_hashes(self.reason) {
            append_uint256_array(&mut bytes, &self.rejected_hashes);
        }
        bytes
    }

    /// Deserializes a `ChangeView` message body (header fields passed in),
    /// matching C# `ChangeView.Deserialize` (v3.10.0): `Timestamp (u64 LE) +
    /// Reason (u8)`, followed by `RejectedHashes` (`UInt256[]`, capped at
    /// `ushort.MaxValue`) ONLY when `reason` is `TxRejectedByPolicy`/`TxInvalid`.
    /// For all other reasons `rejected_hashes` is empty.
    pub fn deserialize(
        data: &[u8],
        block_index: u32,
        view_number: u8,
        validator_index: u8,
    ) -> ConsensusResult<Self> {
        let mut reader = MemoryReader::new(data);

        let timestamp = reader
            .read_u64()
            .map_err(|_| crate::ConsensusError::invalid_proposal("ChangeView message too short"))?;
        let reason_byte = reader
            .read_u8()
            .map_err(|_| crate::ConsensusError::invalid_proposal("ChangeView message too short"))?;
        let reason = ChangeViewReason::from_byte(reason_byte).unwrap_or(ChangeViewReason::Timeout);

        let rejected_hashes = if Self::reason_carries_rejected_hashes(reason) {
            SerializeHelper::deserialize_array::<UInt256>(&mut reader, u16::MAX as usize).map_err(
                |_| crate::ConsensusError::invalid_proposal("ChangeView rejected hashes"),
            )?
        } else {
            Vec::new()
        };

        Ok(Self {
            block_index,
            view_number,
            validator_index,
            timestamp,
            reason,
            rejected_hashes,
        })
    }

    /// Encoded size of the message body (matches C# `ChangeView.Size` minus the
    /// common header): `8 (Timestamp) + 1 (Reason)` plus, for reasons
    /// `TxRejectedByPolicy`/`TxInvalid`, the var-size of the `RejectedHashes`
    /// `UInt256[]`.
    #[must_use]
    pub fn size(&self) -> usize {
        let mut size = 8 + 1;
        if Self::reason_carries_rejected_hashes(self.reason) {
            size += SerializeHelper::get_var_size_serializable_slice(&self.rejected_hashes);
        }
        size
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
