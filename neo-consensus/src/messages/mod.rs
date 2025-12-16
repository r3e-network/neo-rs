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
use neo_io::BinaryWriter;
use neo_primitives::{UInt160, UInt256};
use tracing::warn;

const CONSENSUS_MESSAGE_HEADER_SIZE: usize = 1 + 4 + 1 + 1;

/// Parses the common consensus message header used by Neo N3 dBFT messages.
///
/// Wire format (matches `Neo.Plugins.DBFTPlugin.Messages.ConsensusMessage`):
/// - type (u8)
/// - block_index (u32 LE)
/// - validator_index (u8)
/// - view_number (u8)
pub fn parse_consensus_message_header(
    data: &[u8],
) -> ConsensusResult<(ConsensusMessageType, u32, u8, u8, &[u8])> {
    if data.len() < CONSENSUS_MESSAGE_HEADER_SIZE {
        return Err(crate::ConsensusError::invalid_proposal(
            "consensus message too short",
        ));
    }

    let msg_type =
        ConsensusMessageType::from_byte(data[0]).ok_or_else(|| {
            crate::ConsensusError::invalid_proposal("invalid consensus message type")
        })?;
    let block_index = u32::from_le_bytes(
        data[1..5].try_into().unwrap_or([0u8; 4]),
    );
    let validator_index = data[5];
    let view_number = data[6];
    Ok((msg_type, block_index, validator_index, view_number, &data[7..]))
}

pub fn serialize_consensus_message_header(
    msg_type: ConsensusMessageType,
    block_index: u32,
    validator_index: u8,
    view_number: u8,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(CONSENSUS_MESSAGE_HEADER_SIZE);
    out.push(msg_type.to_byte());
    out.extend_from_slice(&block_index.to_le_bytes());
    out.push(validator_index);
    out.push(view_number);
    out
}

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

/// Envelope wrapping a consensus message for network transport.
///
/// On Neo N3, dBFT messages are relayed over the P2P network via
/// `ExtensiblePayload` with:
/// - `Category = "dBFT"`
/// - `ValidBlockStart = 0`
/// - `ValidBlockEnd = message.BlockIndex`
/// - `Sender = Contract.CreateSignatureRedeemScript(validatorPubKey).ToScriptHash()`
/// - `Data = message.ToArray()`
/// - Witness is a standard single-signature witness verifying `GetSignData(network)` where
///   `GetSignData(network) = networkMagicLE || sha256(unsignedPayloadBytes)`
///
/// This struct mirrors those fields while keeping the consensus crate independent of `neo-core`.
#[derive(Debug, Clone)]
pub struct ConsensusPayload {
    /// Network magic number
    pub network: u32,
    /// Extensible payload category (must be `"dBFT"` for consensus).
    pub category: String,
    /// Valid block start (inclusive).
    pub valid_block_start: u32,
    /// Valid block end (exclusive).
    pub valid_block_end: u32,
    /// Sender script hash (signature contract hash).
    pub sender: UInt160,
    /// Consensus message type (derived from the consensus message itself).
    pub message_type: ConsensusMessageType,
    /// Consensus message block index (derived from the consensus message itself).
    pub block_index: u32,
    /// Consensus message validator index (derived from the consensus message itself).
    pub validator_index: u8,
    /// Consensus message view number (derived from the consensus message itself).
    pub view_number: u8,
    /// Serialized consensus message bytes (`message.ToArray()` in C#).
    pub data: Vec<u8>,
    /// Witness signature (64 bytes, r||s) over `get_sign_data()`.
    pub witness: Vec<u8>,
    /// Cached hash of the unsigned extensible payload bytes.
    pub(crate) cached_hash: Option<UInt256>,
}

impl ConsensusPayload {
    /// Creates a new consensus payload (typically for tests or for bridging from P2P).
    pub fn new(
        network: u32,
        block_index: u32,
        validator_index: u8,
        view_number: u8,
        message_type: ConsensusMessageType,
        data: Vec<u8>,
        sender: UInt160,
    ) -> Self {
        Self {
            network,
            category: "dBFT".to_string(),
            valid_block_start: 0,
            valid_block_end: block_index,
            sender,
            block_index,
            validator_index,
            view_number,
            message_type,
            data,
            witness: Vec::new(),
            cached_hash: None,
        }
    }

    /// Returns the hash of the unsigned extensible payload bytes (`sha256(unsigned)`).
    pub fn hash(&mut self) -> UInt256 {
        if let Some(hash) = self.cached_hash {
            return hash;
        }

        let unsigned = self.unsigned_payload_bytes();
        let digest = neo_crypto::Crypto::sha256(&unsigned);
        let hash = UInt256::from_bytes(&digest).unwrap_or_default();
        self.cached_hash = Some(hash);
        hash
    }

    /// Computes the sign data for this payload (`networkMagicLE || payloadHash`),
    /// mirroring `Neo.Network.P2P.Helper.GetSignData`.
    pub fn get_sign_data(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&self.network.to_le_bytes());
        let mut clone = self.clone();
        data.extend_from_slice(&clone.hash().as_bytes());
        data
    }

    /// Sets the witness (signature)
    pub fn set_witness(&mut self, witness: Vec<u8>) {
        self.witness = witness;
    }

    /// Constructs a consensus payload from the fields of an ExtensiblePayload.
    ///
    /// This is used by the node runtime to bridge inbound P2P messages into the
    /// consensus state machine without requiring `neo-consensus` to depend on `neo-core`.
    pub fn from_extensible_parts(
        network: u32,
        category: String,
        valid_block_start: u32,
        valid_block_end: u32,
        sender: UInt160,
        data: Vec<u8>,
        witness: Vec<u8>,
    ) -> ConsensusResult<Self> {
        let (message_type, block_index, validator_index, view_number, _) =
            parse_consensus_message_header(&data)?;

        Ok(Self {
            network,
            category,
            valid_block_start,
            valid_block_end,
            sender,
            message_type,
            block_index,
            validator_index,
            view_number,
            data,
            witness,
            cached_hash: None,
        })
    }

    fn unsigned_payload_bytes(&self) -> Vec<u8> {
        const MAX_CATEGORY_LEN: usize = 32;
        const MAX_DATA_LEN: usize = 0x0100_0000; // 16MB

        if self.category.len() > MAX_CATEGORY_LEN {
            warn!(
                category_len = self.category.len(),
                "consensus payload category too long"
            );
        }
        if self.data.len() > MAX_DATA_LEN {
            warn!(data_len = self.data.len(), "consensus payload data too long");
        }

        let mut writer = BinaryWriter::new();
        // Matches ExtensiblePayload unsigned serialization in neo-core:
        // category (varstring), valid_block_start (u32), valid_block_end (u32), sender (20 bytes), data (varbytes)
        let _ = writer.write_var_string(&self.category);
        let _ = writer.write_u32(self.valid_block_start);
        let _ = writer.write_u32(self.valid_block_end);
        let _ = writer.write_bytes(&self.sender.to_bytes());
        let _ = writer.write_var_bytes(&self.data);
        writer.into_bytes()
    }
}
