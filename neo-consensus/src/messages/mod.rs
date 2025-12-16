//! Consensus message definitions for dBFT protocol.

mod change_view;
mod commit;
mod prepare_request;
mod prepare_response;
mod recovery;

pub use change_view::ChangeViewMessage;
pub use commit::CommitMessage;
pub use prepare_request::PrepareRequestMessage;
pub use prepare_response::PrepareResponseMessage;
pub use recovery::{RecoveryMessage, RecoveryRequestMessage};

use crate::{ConsensusMessageType, ConsensusResult};

/// Common trait for all consensus messages
pub trait ConsensusMessage: Send + Sync {
    /// Returns the message type
    fn message_type(&self) -> ConsensusMessageType;

    /// Returns the block index this message is for
    fn block_index(&self) -> u32;

    /// Returns the validator index of the sender
    fn validator_index(&self) -> u8;

    /// Returns the view number
    fn view_number(&self) -> u8;

    /// Serializes the message to bytes
    fn serialize(&self) -> Vec<u8>;

    /// Validates the message
    fn validate(&self) -> ConsensusResult<()>;
}

/// Envelope wrapping any consensus message with metadata
#[derive(Debug, Clone)]
pub struct ConsensusPayload {
    /// Network magic number
    pub network: u32,
    /// Block index
    pub block_index: u32,
    /// Validator index
    pub validator_index: u8,
    /// View number
    pub view_number: u8,
    /// Message type
    pub message_type: ConsensusMessageType,
    /// Serialized message data
    pub data: Vec<u8>,
    /// Witness (signature)
    pub witness: Vec<u8>,
}

impl ConsensusPayload {
    /// Creates a new consensus payload
    pub fn new(
        network: u32,
        block_index: u32,
        validator_index: u8,
        view_number: u8,
        message_type: ConsensusMessageType,
        data: Vec<u8>,
    ) -> Self {
        Self {
            network,
            block_index,
            validator_index,
            view_number,
            message_type,
            data,
            witness: Vec::new(),
        }
    }

    /// Computes the hash of this payload for signing
    pub fn get_sign_data(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&self.network.to_le_bytes());
        data.extend_from_slice(&self.block_index.to_le_bytes());
        data.push(self.validator_index);
        data.push(self.view_number);
        data.push(self.message_type.to_byte());
        data.extend_from_slice(&self.data);
        data
    }

    /// Serializes this consensus message using the Neo N3 DBFTPlugin on-wire format:
    /// `[type:1][block_index:4][validator_index:1][view_number:1][body...]`.
    ///
    /// This is the byte array stored in `ExtensiblePayload.Data` for category `"dBFT"`.
    pub fn to_message_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(1 + 4 + 1 + 1 + self.data.len());
        bytes.push(self.message_type.to_byte());
        bytes.extend_from_slice(&self.block_index.to_le_bytes());
        bytes.push(self.validator_index);
        bytes.push(self.view_number);
        bytes.extend_from_slice(&self.data);
        bytes
    }

    /// Parses a consensus message from DBFTPlugin on-wire bytes.
    pub fn from_message_bytes(
        network: u32,
        message_bytes: &[u8],
        witness: Vec<u8>,
    ) -> ConsensusResult<Self> {
        if message_bytes.len() < 1 + 4 + 1 + 1 {
            return Err(crate::ConsensusError::invalid_proposal(
                "Consensus message too short",
            ));
        }

        let message_type = ConsensusMessageType::from_byte(message_bytes[0]).ok_or_else(|| {
            crate::ConsensusError::invalid_proposal("Invalid consensus message type")
        })?;
        let block_index = u32::from_le_bytes(message_bytes[1..5].try_into().unwrap_or([0u8; 4]));
        let validator_index = message_bytes[5];
        let view_number = message_bytes[6];
        let data = message_bytes[7..].to_vec();

        Ok(Self {
            network,
            block_index,
            validator_index,
            view_number,
            message_type,
            data,
            witness,
        })
    }

    /// Sets the witness (signature)
    pub fn set_witness(&mut self, witness: Vec<u8>) {
        self.witness = witness;
    }
}

/// Builds DBFTPlugin consensus message bytes:
/// `[type:1][block_index:4][validator_index:1][view_number:1][body...]`.
pub(crate) fn consensus_message_bytes(
    message_type: crate::ConsensusMessageType,
    block_index: u32,
    validator_index: u8,
    view_number: u8,
    body: &[u8],
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(1 + 4 + 1 + 1 + body.len());
    bytes.push(message_type.to_byte());
    bytes.extend_from_slice(&block_index.to_le_bytes());
    bytes.push(validator_index);
    bytes.push(view_number);
    bytes.extend_from_slice(body);
    bytes
}

#[cfg(test)]
mod wire_format_tests {
    use super::*;
    use neo_primitives::UInt256;

    #[test]
    fn consensus_payload_to_message_bytes_layout() {
        let hash = UInt256::from([0xAB; 32]);
        let payload = ConsensusPayload::new(
            0x4E454F,
            42,
            9,
            1,
            ConsensusMessageType::PrepareResponse,
            hash.to_array().to_vec(),
        );

        let bytes = payload.to_message_bytes();
        assert_eq!(bytes[0], ConsensusMessageType::PrepareResponse.to_byte());
        assert_eq!(&bytes[1..5], &42u32.to_le_bytes());
        assert_eq!(bytes[5], 9);
        assert_eq!(bytes[6], 1);
        assert_eq!(&bytes[7..], &hash.to_array());
    }
}
