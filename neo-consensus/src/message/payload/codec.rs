use alloc::vec::Vec;

use neo_base::{
    encoding::{read_varint, write_varint, DecodeError},
    hash::Hash256,
    NeoDecode, NeoEncode, NeoRead, NeoWrite,
};

use crate::message::{
    payload::ConsensusMessage,
    types::{ChangeViewReason, MessageKind, ViewNumber},
};

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
            Self::ChangeView {
                new_view,
                reason,
                timestamp_ms,
            } => {
                new_view.neo_encode(writer);
                writer.write_u8(*reason as u8);
                timestamp_ms.neo_encode(writer);
            }
        }
    }
}

impl NeoDecode for ConsensusMessage {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
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
                let timestamp_ms = u64::neo_decode(reader)?;
                ConsensusMessage::ChangeView {
                    new_view: view,
                    reason,
                    timestamp_ms,
                }
            }
        })
    }
}
