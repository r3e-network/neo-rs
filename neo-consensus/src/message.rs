use alloc::vec::Vec;
use core::convert::TryFrom;

use neo_base::{hash::Hash256, read_varint, write_varint, NeoDecode, NeoEncode, NeoRead, NeoWrite};
use neo_crypto::SignatureBytes;
use serde::{Deserialize, Serialize};

use crate::validator::ValidatorId;

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct ViewNumber(pub u32);

impl ViewNumber {
    pub const ZERO: Self = Self(0);
}

impl NeoEncode for ViewNumber {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_u32(self.0);
    }
}

impl NeoDecode for ViewNumber {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, neo_base::encoding::DecodeError> {
        let value = reader.read_u32()?;
        Ok(ViewNumber(value))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsensusMessage {
    PrepareRequest {
        proposal_hash: Hash256,
        height: u64,
        tx_hashes: Vec<Hash256>,
    },
    PrepareResponse {
        proposal_hash: Hash256,
    },
    Commit {
        proposal_hash: Hash256,
    },
    ChangeView {
        new_view: ViewNumber,
        reason: ChangeViewReason,
    },
}

impl ConsensusMessage {
    pub fn kind(&self) -> MessageKind {
        match self {
            Self::PrepareRequest { .. } => MessageKind::PrepareRequest,
            Self::PrepareResponse { .. } => MessageKind::PrepareResponse,
            Self::Commit { .. } => MessageKind::Commit,
            Self::ChangeView { .. } => MessageKind::ChangeView,
        }
    }

    pub fn proposal_hash(&self) -> Option<Hash256> {
        match self {
            Self::PrepareRequest { proposal_hash, .. }
            | Self::PrepareResponse { proposal_hash }
            | Self::Commit { proposal_hash } => Some(*proposal_hash),
            Self::ChangeView { .. } => None,
        }
    }
}

impl NeoEncode for ConsensusMessage {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_u8(self.kind().as_u8());
        match self {
            Self::PrepareRequest {
                proposal_hash,
                height,
                tx_hashes,
            } => {
                proposal_hash.neo_encode(writer);
                height.neo_encode(writer);
                write_varint(writer, tx_hashes.len() as u64);
                for hash in tx_hashes {
                    hash.neo_encode(writer);
                }
            }
            Self::PrepareResponse { proposal_hash } | Self::Commit { proposal_hash } => {
                proposal_hash.neo_encode(writer);
            }
            Self::ChangeView { new_view, reason } => {
                new_view.neo_encode(writer);
                writer.write_u8(*reason as u8);
            }
        }
    }
}

impl NeoDecode for ConsensusMessage {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, neo_base::encoding::DecodeError> {
        let kind = reader.read_u8()?;
        Ok(match MessageKind::try_from(kind)? {
            MessageKind::PrepareRequest => {
                let proposal_hash = Hash256::neo_decode(reader)?;
                let height = u64::neo_decode(reader)?;
                let count = read_varint(reader)? as usize;
                let mut hashes = Vec::with_capacity(count);
                for _ in 0..count {
                    hashes.push(Hash256::neo_decode(reader)?);
                }
                ConsensusMessage::PrepareRequest {
                    proposal_hash,
                    height,
                    tx_hashes: hashes,
                }
            }
            MessageKind::PrepareResponse => ConsensusMessage::PrepareResponse {
                proposal_hash: Hash256::neo_decode(reader)?,
            },
            MessageKind::Commit => ConsensusMessage::Commit {
                proposal_hash: Hash256::neo_decode(reader)?,
            },
            MessageKind::ChangeView => {
                let view = ViewNumber::neo_decode(reader)?;
                let reason = ChangeViewReason::from_u8(reader.read_u8()?)?;
                ConsensusMessage::ChangeView {
                    new_view: view,
                    reason,
                }
            }
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedMessage {
    pub height: u64,
    pub view: ViewNumber,
    pub validator: ValidatorId,
    pub message: ConsensusMessage,
    pub signature: SignatureBytes,
}

impl SignedMessage {
    pub fn new(
        height: u64,
        view: ViewNumber,
        validator: ValidatorId,
        message: ConsensusMessage,
        signature: SignatureBytes,
    ) -> Self {
        Self {
            height,
            view,
            validator,
            message,
            signature,
        }
    }

    pub fn kind(&self) -> MessageKind {
        self.message.kind()
    }

    pub fn digest(&self) -> Hash256 {
        let mut buf = Vec::new();
        UnsignedEnvelope::from(self).neo_encode(&mut buf);
        Hash256::new(neo_base::double_sha256(buf))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum MessageKind {
    PrepareRequest = 0,
    PrepareResponse = 1,
    Commit = 2,
    ChangeView = 3,
}

impl MessageKind {
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

impl TryFrom<u8> for MessageKind {
    type Error = neo_base::encoding::DecodeError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::PrepareRequest),
            1 => Ok(Self::PrepareResponse),
            2 => Ok(Self::Commit),
            3 => Ok(Self::ChangeView),
            _ => Err(neo_base::encoding::DecodeError::InvalidValue(
                "message kind",
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum ChangeViewReason {
    Timeout = 0,
    InvalidProposal = 1,
    Manual = 2,
}

impl ChangeViewReason {
    fn from_u8(value: u8) -> Result<Self, neo_base::encoding::DecodeError> {
        match value {
            0 => Ok(Self::Timeout),
            1 => Ok(Self::InvalidProposal),
            2 => Ok(Self::Manual),
            _ => Err(neo_base::encoding::DecodeError::InvalidValue("change view")),
        }
    }
}

struct UnsignedEnvelope<'a> {
    height: u64,
    view: ViewNumber,
    validator: ValidatorId,
    message: &'a ConsensusMessage,
}

impl<'a> From<&'a SignedMessage> for UnsignedEnvelope<'a> {
    fn from(value: &'a SignedMessage) -> Self {
        Self {
            height: value.height,
            view: value.view,
            validator: value.validator,
            message: &value.message,
        }
    }
}

impl<'a> NeoEncode for UnsignedEnvelope<'a> {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.height.neo_encode(writer);
        self.view.neo_encode(writer);
        self.validator.neo_encode(writer);
        self.message.neo_encode(writer);
    }
}

impl NeoEncode for SignedMessage {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        UnsignedEnvelope::from(self).neo_encode(writer);
        self.signature.neo_encode(writer);
    }
}

impl NeoDecode for SignedMessage {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, neo_base::encoding::DecodeError> {
        let height = u64::neo_decode(reader)?;
        let view = ViewNumber::neo_decode(reader)?;
        let validator = ValidatorId::neo_decode(reader)?;
        let message = ConsensusMessage::neo_decode(reader)?;
        let signature = SignatureBytes::neo_decode(reader)?;
        Ok(Self {
            height,
            view,
            validator,
            message,
            signature,
        })
    }
}
