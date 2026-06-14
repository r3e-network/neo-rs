//! Strongly-typed [`ProtocolMessage`] enum and codecs.
//!
//! `ProtocolMessage` is the typed view of the wire-level
//! [`crate::wire::Message`]: it carries a `command` discriminator plus the
//! decoded payload of the relevant kind. The wire-format conversion
//! handled by [`Self::from_bytes`] / [`Self::to_bytes`] mirrors the
//! C# `Neo.Network.P2P.Message` round-trip.

use super::error::{WireError, WireResult};
use neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_p2p::MessageCommand;
use neo_payloads::p2p_payloads::{
    AddrPayload, FilterAddPayload, FilterLoadPayload, GetBlockByIndexPayload, GetBlocksPayload,
    InvPayload, PingPayload, VersionPayload,
};
use neo_payloads::{Block, ExtensiblePayload, HeadersPayload, MerkleBlockPayload, Transaction};

/// Strongly-typed representation of every payload carried by the Neo P2P protocol.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum ProtocolMessage {
    /// Version handshake.
    Version(VersionPayload),
    /// Version acknowledgement.
    Verack,
    /// Request peer addresses.
    GetAddr,
    /// Peer address list.
    Addr(AddrPayload),
    /// Keepalive ping.
    Ping(PingPayload),
    /// Keepalive pong.
    Pong(PingPayload),
    /// Request block headers.
    GetHeaders(GetBlockByIndexPayload),
    /// Block headers.
    Headers(HeadersPayload),
    /// Request blocks.
    GetBlocks(GetBlocksPayload),
    /// Request mempool contents.
    Mempool,
    /// Inventory announcement.
    Inv(InvPayload),
    /// Inventory data request.
    GetData(InvPayload),
    /// Block-by-index data request.
    GetBlockByIndex(GetBlockByIndexPayload),
    /// Inventory not-found response.
    NotFound(InvPayload),
    /// Transaction broadcast.
    Transaction(Transaction),
    /// Block broadcast.
    Block(Block),
    /// Extensible payload (e.g. dBFT consensus, state-root votes).
    Extensible(ExtensiblePayload),
    /// Bloom-filter load.
    FilterLoad(FilterLoadPayload),
    /// Bloom-filter add.
    FilterAdd(FilterAddPayload),
    /// Bloom-filter clear.
    FilterClear,
    /// Merkle block response (filtered block).
    MerkleBlock(MerkleBlockPayload),
    /// Reserved alert command (raw bytes).
    Alert(Vec<u8>),
    /// Reserved reject command (raw bytes).
    Reject(Vec<u8>),
    /// Unknown/forward-compatible command.
    Unknown {
        /// The command byte received on the wire.
        command: MessageCommand,
        /// The opaque payload bytes.
        bytes: Vec<u8>,
    },
}

impl ProtocolMessage {
    /// Returns the wire command associated with this payload variant.
    pub fn command(&self) -> MessageCommand {
        match self {
            Self::Version(_) => MessageCommand::Version,
            Self::Verack => MessageCommand::Verack,
            Self::GetAddr => MessageCommand::GetAddr,
            Self::Addr(_) => MessageCommand::Addr,
            Self::Ping(_) => MessageCommand::Ping,
            Self::Pong(_) => MessageCommand::Pong,
            Self::GetHeaders(_) => MessageCommand::GetHeaders,
            Self::Headers(_) => MessageCommand::Headers,
            Self::GetBlocks(_) => MessageCommand::GetBlocks,
            Self::Mempool => MessageCommand::Mempool,
            Self::Inv(_) => MessageCommand::Inv,
            Self::GetData(_) => MessageCommand::GetData,
            Self::GetBlockByIndex(_) => MessageCommand::GetBlockByIndex,
            Self::NotFound(_) => MessageCommand::NotFound,
            Self::Transaction(_) => MessageCommand::Transaction,
            Self::Block(_) => MessageCommand::Block,
            Self::Extensible(_) => MessageCommand::Extensible,
            Self::FilterLoad(_) => MessageCommand::FilterLoad,
            Self::FilterAdd(_) => MessageCommand::FilterAdd,
            Self::FilterClear => MessageCommand::FilterClear,
            Self::MerkleBlock(_) => MessageCommand::MerkleBlock,
            Self::Alert(_) => MessageCommand::Alert,
            Self::Reject(_) => MessageCommand::Reject,
            Self::Unknown { command, .. } => *command,
        }
    }

    /// Returns whether the wire format should attempt LZ4 compression
    /// for this payload variant. Mirrors the C# `MessageCommand`
    /// compression metadata.
    pub fn allows_compression(&self) -> bool {
        !matches!(self, Self::Unknown { .. }) && self.command().allows_compression()
    }

    /// Serialises the typed payload to its on-the-wire byte sequence.
    pub fn serialize_payload(&self) -> WireResult<Vec<u8>> {
        match self {
            Self::Version(p) => serialize_payload(p),
            Self::Addr(p) => serialize_payload(p),
            Self::Ping(p) => serialize_payload(p),
            Self::Pong(p) => serialize_payload(p),
            Self::GetHeaders(p) => serialize_payload(p),
            Self::Headers(p) => serialize_payload(p),
            Self::GetBlocks(p) => serialize_payload(p),
            Self::Inv(p) => serialize_payload(p),
            Self::GetData(p) => serialize_payload(p),
            Self::GetBlockByIndex(p) => serialize_payload(p),
            Self::NotFound(p) => serialize_payload(p),
            Self::Transaction(p) => serialize_payload(p),
            Self::Block(p) => serialize_payload(p),
            Self::Extensible(p) => serialize_payload(p),
            Self::FilterLoad(p) => serialize_payload(p),
            Self::FilterAdd(p) => serialize_payload(p),
            Self::MerkleBlock(p) => serialize_payload(p),
            Self::Verack | Self::GetAddr | Self::Mempool | Self::FilterClear => Ok(Vec::new()),
            Self::Alert(bytes) | Self::Reject(bytes) => Ok(bytes.clone()),
            Self::Unknown { bytes, .. } => Ok(bytes.clone()),
        }
    }

    /// Deserialises the typed payload from a wire-format byte slice.
    pub fn deserialize_payload(command: MessageCommand, data: &[u8]) -> WireResult<Self> {
        let msg = match command {
            MessageCommand::Version => Self::Version(deserialize_payload(data)?),
            MessageCommand::Verack => {
                ensure_empty(command, data)?;
                Self::Verack
            }
            MessageCommand::GetAddr => {
                ensure_empty(command, data)?;
                Self::GetAddr
            }
            MessageCommand::Addr => Self::Addr(deserialize_payload(data)?),
            MessageCommand::Ping => Self::Ping(deserialize_payload(data)?),
            MessageCommand::Pong => Self::Pong(deserialize_payload(data)?),
            MessageCommand::GetHeaders => Self::GetHeaders(deserialize_payload(data)?),
            MessageCommand::Headers => Self::Headers(deserialize_payload(data)?),
            MessageCommand::GetBlocks => Self::GetBlocks(deserialize_payload(data)?),
            MessageCommand::Mempool => {
                ensure_empty(command, data)?;
                Self::Mempool
            }
            MessageCommand::Inv => Self::Inv(deserialize_payload(data)?),
            MessageCommand::GetData => Self::GetData(deserialize_payload(data)?),
            MessageCommand::GetBlockByIndex => Self::GetBlockByIndex(deserialize_payload(data)?),
            MessageCommand::NotFound => Self::NotFound(deserialize_payload(data)?),
            MessageCommand::Transaction => Self::Transaction(deserialize_payload(data)?),
            MessageCommand::Block => Self::Block(deserialize_payload(data)?),
            MessageCommand::Extensible => Self::Extensible(deserialize_payload(data)?),
            MessageCommand::FilterLoad => Self::FilterLoad(deserialize_payload(data)?),
            MessageCommand::FilterAdd => Self::FilterAdd(deserialize_payload(data)?),
            MessageCommand::FilterClear => {
                ensure_empty(command, data)?;
                Self::FilterClear
            }
            MessageCommand::MerkleBlock => Self::MerkleBlock(deserialize_payload(data)?),
            MessageCommand::Alert => Self::Alert(data.to_vec()),
            MessageCommand::Reject => Self::Reject(data.to_vec()),
            MessageCommand::Unknown(value) => Self::Unknown {
                command: MessageCommand::Unknown(value),
                bytes: data.to_vec(),
            },
        };
        Ok(msg)
    }

    /// Convenience constructor for pong replies.
    pub fn pong(nonce: u32) -> Self {
        Self::Pong(PingPayload::create_with_nonce(0, nonce))
    }

    /// Convenience constructor for pong replies with a specific block index.
    pub fn pong_with_block_index(block_index: u32, nonce: u32) -> Self {
        Self::Pong(PingPayload::create_with_nonce(block_index, nonce))
    }
}

fn serialize_payload<T: Serializable>(payload: &T) -> WireResult<Vec<u8>> {
    let mut writer = BinaryWriter::with_capacity(payload.size());
    Serializable::serialize(payload, &mut writer)?;
    Ok(writer.into_bytes())
}

fn deserialize_payload<T: Serializable>(bytes: &[u8]) -> WireResult<T> {
    let mut reader = MemoryReader::new(bytes);
    let payload = T::deserialize(&mut reader).map_err(WireError::from)?;
    if reader.remaining() != 0 {
        return Err(WireError::InvalidMessage(format!(
            "trailing {} bytes after payload",
            reader.remaining()
        )));
    }
    Ok(payload)
}

fn ensure_empty(command: MessageCommand, bytes: &[u8]) -> WireResult<()> {
    if bytes.is_empty() {
        Ok(())
    } else {
        Err(WireError::InvalidMessage(format!(
            "command {command:?} does not carry a payload but {} byte(s) supplied",
            bytes.len()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_message_command_matches_variant() {
        assert_eq!(ProtocolMessage::Verack.command(), MessageCommand::Verack);
        assert_eq!(ProtocolMessage::pong(42).command(), MessageCommand::Pong);
    }

    #[test]
    fn empty_command_round_trip() {
        let payload = ProtocolMessage::Verack
            .serialize_payload()
            .expect("serialize");
        assert!(payload.is_empty());
        let decoded = ProtocolMessage::deserialize_payload(MessageCommand::Verack, &payload)
            .expect("deserialize");
        matches!(decoded, ProtocolMessage::Verack);
    }

    #[test]
    fn ping_round_trip() {
        let ping = ProtocolMessage::Ping(PingPayload::create(7));
        let bytes = ping.serialize_payload().expect("serialize");
        let decoded = ProtocolMessage::deserialize_payload(MessageCommand::Ping, &bytes)
            .expect("deserialize");
        match decoded {
            ProtocolMessage::Ping(p) => assert_eq!(p.last_block_index, 7),
            other => panic!("unexpected variant: {other:?}"),
        }
    }
}
