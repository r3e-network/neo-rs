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

    /// Sets the witness (signature)
    pub fn set_witness(&mut self, witness: Vec<u8>) {
        self.witness = witness;
    }
}
