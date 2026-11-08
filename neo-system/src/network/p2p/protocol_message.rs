//! Strongly-typed protocol message vocabulary and network message framing.
//!
//! Mirrors `Neo.Network.P2P.Messages.ProtocolMessage` and `NetworkMessage`.
//! This module is the central dispatch type: every decoded inbound frame
//! becomes a [`ProtocolMessage`] variant, and every outbound response is
//! constructed from one.
//!
//! # Dependencies
//!
//! Only `neo-protocol` canonical payloads (`Block`, `Header`, `Transaction`,
//! `Extensible`), this crate's own payload types, `neo-io`, and `neo-primitives`.

use super::message::Message;
use super::message_command::MessageCommand;
use super::message_flags::MessageFlags;
use super::payloads::{
    AddrPayload, FilterAddPayload, FilterLoadPayload, GetBlockByIndexPayload, GetBlocksPayload,
    InvPayload, PingPayload, VersionPayload,
};
use super::payloads::headers_payload::HeadersPayload;
use super::payloads::merkle_block_payload::MerkleBlockPayload;
use neo_io_crate::{BinaryWriter, MemoryReader, Serializable};
use neo_protocol::payloads::{Block, Extensible, Transaction};

// ---------------------------------------------------------------------------
// MessageHeader
// ---------------------------------------------------------------------------

/// Header metadata attached to every P2P message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageHeader {
    /// Command opcode that identifies the payload type.
    pub command: MessageCommand,
}

// ---------------------------------------------------------------------------
// ProtocolMessage
// ---------------------------------------------------------------------------

/// Strongly-typed representation of every payload carried by the Neo P2P protocol.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum ProtocolMessage {
    /// Version handshake payload exchanged when a connection is established.
    Version(VersionPayload),
    /// Acknowledgement of a peer's version handshake.
    Verack,
    /// Request for the peer's known node addresses.
    GetAddr,
    /// Response carrying a batch of node addresses.
    Addr(AddrPayload),
    /// Latency probe sent to a connected peer.
    Ping(PingPayload),
    /// Reply to a `Ping` latency probe.
    Pong(PingPayload),
    /// Request for block headers starting at a given height.
    GetHeaders(GetBlockByIndexPayload),
    /// Response carrying a batch of block headers.
    Headers(HeadersPayload),
    /// Request for full blocks identified by hash.
    GetBlocks(GetBlocksPayload),
    /// Request for the contents of the peer's memory pool.
    Mempool,
    /// Inventory announcement of known blocks or transactions.
    Inv(InvPayload),
    /// Request for payloads advertised by an inventory message.
    GetData(InvPayload),
    /// Request for a single block identified by index.
    GetBlockByIndex(GetBlockByIndexPayload),
    /// Notice that one or more requested payloads are unavailable.
    NotFound(InvPayload),
    /// A transaction broadcast across the network.
    Transaction(Transaction),
    /// A consensus-approved block.
    Block(Block),
    /// Extensible payload carrying consensus or plugin data.
    Extensible(Extensible),
    /// Bloom filter configuration applied to a peer connection.
    FilterLoad(FilterLoadPayload),
    /// Entry added to the peer's bloom filter.
    FilterAdd(FilterAddPayload),
    /// Request to clear the peer's bloom filter.
    FilterClear,
    /// Filtered block containing only the matching transactions.
    MerkleBlock(MerkleBlockPayload),
    /// Deprecated administrative alert message (raw bytes).
    Alert(Vec<u8>),
    /// Deprecated rejection notice for refused messages (raw bytes).
    Reject(Vec<u8>),
    /// Unrecognized message preserved for diagnostics or forwarding.
    Unknown {
        /// Command observed on the wire.
        command: MessageCommand,
        /// Raw bytes of the unrecognized payload.
        bytes: Vec<u8>,
    },
}

impl ProtocolMessage {
    /// Returns the underlying command associated with this payload.
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

    /// Convenience constructor for pong replies.
    pub fn pong(nonce: u32) -> Self {
        Self::pong_with_block_index(0, nonce)
    }

    /// Creates a pong reply with a specific block index and nonce.
    pub fn pong_with_block_index(block_index: u32, nonce: u32) -> Self {
        Self::Pong(PingPayload::create_with_nonce(block_index, nonce))
    }

    /// Whether this payload should attempt compression when serialized.
    pub fn should_try_compress(&self) -> bool {
        !matches!(self, Self::Unknown { .. }) && self.command().allows_compression()
    }

    /// Serializes the payload into its binary representation.
    pub fn to_bytes(&self) -> P2PResult<Vec<u8>> {
        self.serialize_payload()
    }

    /// Reconstructs the payload from its binary form and command discriminator.
    pub fn from_bytes(command: MessageCommand, data: &[u8]) -> P2PResult<Self> {
        Self::deserialize_payload(command, data)
    }

    fn serialize_payload(&self) -> P2PResult<Vec<u8>> {
        match self {
            Self::Version(p) => serialize_typed(p),
            Self::Addr(p) => serialize_typed(p),
            Self::Ping(p) => serialize_typed(p),
            Self::Pong(p) => serialize_typed(p),
            Self::GetHeaders(p) => serialize_typed(p),
            Self::Headers(p) => serialize_typed(p),
            Self::GetBlocks(p) => serialize_typed(p),
            Self::Inv(p) => serialize_typed(p),
            Self::GetData(p) => serialize_typed(p),
            Self::GetBlockByIndex(p) => serialize_typed(p),
            Self::NotFound(p) => serialize_typed(p),
            Self::Transaction(p) => serialize_typed(p),
            Self::Block(p) => serialize_typed(p),
            Self::Extensible(p) => serialize_typed(p),
            Self::FilterLoad(p) => serialize_typed(p),
            Self::FilterAdd(p) => serialize_typed(p),
            Self::MerkleBlock(p) => serialize_typed(p),
            Self::Verack | Self::GetAddr | Self::Mempool | Self::FilterClear => Ok(Vec::new()),
            Self::Alert(bytes) | Self::Reject(bytes) => Ok(bytes.clone()),
            Self::Unknown { bytes, .. } => Ok(bytes.clone()),
        }
    }

    fn deserialize_payload(command: MessageCommand, data: &[u8]) -> P2PResult<Self> {
        let message = match command {
            MessageCommand::Version => Self::Version(deserialize_typed(data)?),
            MessageCommand::Addr => Self::Addr(deserialize_typed(data)?),
            MessageCommand::Ping => Self::Ping(deserialize_typed(data)?),
            MessageCommand::Pong => Self::Pong(deserialize_typed(data)?),
            MessageCommand::GetHeaders => Self::GetHeaders(deserialize_typed(data)?),
            MessageCommand::Headers => Self::Headers(deserialize_typed(data)?),
            MessageCommand::GetBlocks => Self::GetBlocks(deserialize_typed(data)?),
            MessageCommand::Inv => Self::Inv(deserialize_typed(data)?),
            MessageCommand::GetData => Self::GetData(deserialize_typed(data)?),
            MessageCommand::GetBlockByIndex => Self::GetBlockByIndex(deserialize_typed(data)?),
            MessageCommand::NotFound => Self::NotFound(deserialize_typed(data)?),
            MessageCommand::Transaction => Self::Transaction(deserialize_typed(data)?),
            MessageCommand::Block => Self::Block(deserialize_typed(data)?),
            MessageCommand::Extensible => Self::Extensible(deserialize_typed(data)?),
            MessageCommand::FilterLoad => Self::FilterLoad(deserialize_typed(data)?),
            MessageCommand::FilterAdd => Self::FilterAdd(deserialize_typed(data)?),
            MessageCommand::MerkleBlock => Self::MerkleBlock(deserialize_typed(data)?),
            MessageCommand::Verack => {
                ensure_empty(command, data)?;
                Self::Verack
            }
            MessageCommand::GetAddr => {
                ensure_empty(command, data)?;
                Self::GetAddr
            }
            MessageCommand::Mempool => {
                ensure_empty(command, data)?;
                Self::Mempool
            }
            MessageCommand::FilterClear => {
                ensure_empty(command, data)?;
                Self::FilterClear
            }
            MessageCommand::Alert => Self::Alert(data.to_vec()),
            MessageCommand::Reject => Self::Reject(data.to_vec()),
            MessageCommand::Unknown(value) => Self::Unknown {
                command: MessageCommand::Unknown(value),
                bytes: data.to_vec(),
            },
        };
        Ok(message)
    }
}

// ---------------------------------------------------------------------------
// NetworkMessage
// ---------------------------------------------------------------------------

/// Fully decoded network message combining header metadata and typed payload.
#[derive(Debug, Clone)]
pub struct NetworkMessage {
    /// Header metadata.
    pub header: MessageHeader,
    /// Message flags (e.g. compression state).
    pub flags: MessageFlags,
    /// Strongly typed payload.
    pub payload: ProtocolMessage,
    /// Raw payload bytes as sent on the wire (compressed when flag is set).
    wire_payload: Option<Vec<u8>>,
}

impl NetworkMessage {
    /// Creates a new network message from the supplied payload.
    pub fn new(payload: ProtocolMessage) -> Self {
        let command = payload.command();
        Self {
            header: MessageHeader { command },
            flags: MessageFlags::NONE,
            payload,
            wire_payload: None,
        }
    }

    /// Convenience accessor for the command associated with the payload.
    pub fn command(&self) -> MessageCommand {
        self.header.command
    }

    /// Returns the original wire-format payload if available.
    pub fn wire_payload(&self) -> Option<&[u8]> {
        self.wire_payload.as_deref()
    }

    /// Returns the message encoded as wire bytes (flags + command + payload).
    ///
    /// `allow_compression` mirrors the C# `Message.ToArray(bool)` behaviour.
    pub fn to_bytes(&self, allow_compression: bool) -> P2PResult<Vec<u8>> {
        let payload_bytes = self.payload.to_bytes()?;
        let message = Message::create_from_payload_bytes(
            self.header.command,
            payload_bytes,
            allow_compression && self.payload.should_try_compress(),
        )
        .map_err(|e| P2PMessageError::InvalidMessage(e.to_string()))?;
        message
            .to_bytes(allow_compression)
            .map_err(|e| P2PMessageError::Io(e.to_string()))
    }

    /// Decodes a message from raw wire bytes (flags + command + var payload).
    pub fn from_bytes(bytes: &[u8]) -> P2PResult<Self> {
        let mut reader = MemoryReader::new(bytes);
        let message: crate::message::Message = neo_io_crate::Serializable::deserialize(&mut reader)
            .map_err(|e| P2PMessageError::Io(e.to_string()))?;

        if reader.remaining() != 0 {
            return Err(P2PMessageError::InvalidMessage(
                "trailing data after message".into(),
            ));
        }

        let payload = ProtocolMessage::from_bytes(message.command, message.payload())?;

        Ok(Self {
            header: MessageHeader {
                command: message.command,
            },
            flags: message.flags,
            payload,
            wire_payload: Some(message.payload_compressed),
        })
    }
}

// ---------------------------------------------------------------------------
// Error type for protocol message operations
// ---------------------------------------------------------------------------

/// Error type for protocol message serialization/deserialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum P2PMessageError {
    /// The message bytes are structurally invalid.
    InvalidMessage(String),
    /// An I/O error during serialization.
    Io(String),
}

impl std::fmt::Display for P2PMessageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMessage(msg) => write!(f, "invalid message: {msg}"),
            Self::Io(msg) => write!(f, "io error: {msg}"),
        }
    }
}

impl std::error::Error for P2PMessageError {}

// Conversion to NetworkError for compatibility with neo-system
impl From<P2PMessageError> for neo_primitives::NetworkError {
    fn from(error: P2PMessageError) -> Self {
        match error {
            P2PMessageError::InvalidMessage(msg) => neo_primitives::NetworkError::InvalidMessage(msg),
            P2PMessageError::Io(msg) => neo_primitives::NetworkError::ConnectionError(msg),
        }
    }
}

/// Result alias for protocol message operations.
pub type P2PResult<T> = Result<T, P2PMessageError>;

// ---------------------------------------------------------------------------
// Serialization helpers
// ---------------------------------------------------------------------------

fn serialize_typed<T: Serializable>(payload: &T) -> P2PResult<Vec<u8>> {
    let mut writer = BinaryWriter::with_capacity(payload.size());
    payload
        .serialize(&mut writer)
        .map_err(|e| P2PMessageError::Io(e.to_string()))?;
    Ok(writer.into_bytes())
}

fn deserialize_typed<T: Serializable>(bytes: &[u8]) -> P2PResult<T> {
    let mut reader = MemoryReader::new(bytes);
    let payload =
        T::deserialize(&mut reader).map_err(|e| P2PMessageError::Io(e.to_string()))?;
    if reader.remaining() != 0 {
        return Err(P2PMessageError::InvalidMessage(
            "trailing bytes present after payload deserialization".into(),
        ));
    }
    Ok(payload)
}

fn ensure_empty(command: MessageCommand, bytes: &[u8]) -> P2PResult<()> {
    if bytes.is_empty() {
        Ok(())
    } else {
        Err(P2PMessageError::InvalidMessage(format!(
            "command {:?} does not carry a payload but {} byte(s) were provided",
            command,
            bytes.len()
        )))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_message_command_mapping_is_complete() {
        let cases: Vec<(ProtocolMessage, MessageCommand)> = vec![
            (ProtocolMessage::Verack, MessageCommand::Verack),
            (ProtocolMessage::GetAddr, MessageCommand::GetAddr),
            (ProtocolMessage::Mempool, MessageCommand::Mempool),
            (ProtocolMessage::FilterClear, MessageCommand::FilterClear),
            (
                ProtocolMessage::Alert(vec![1, 2]),
                MessageCommand::Alert,
            ),
            (
                ProtocolMessage::Reject(vec![3]),
                MessageCommand::Reject,
            ),
        ];
        for (msg, expected_cmd) in cases {
            assert_eq!(msg.command(), expected_cmd);
        }
    }

    #[test]
    fn empty_payload_serializes_to_empty_vec() {
        for msg in [
            ProtocolMessage::Verack,
            ProtocolMessage::GetAddr,
            ProtocolMessage::Mempool,
            ProtocolMessage::FilterClear,
        ] {
            assert!(msg.to_bytes().unwrap().is_empty());
        }
    }

    #[test]
    fn raw_payload_roundtrips() {
        let alert_bytes = vec![0xA1, 0xA2, 0xA3];
        let msg = ProtocolMessage::Alert(alert_bytes.clone());
        assert_eq!(msg.to_bytes().unwrap(), alert_bytes);

        let decoded = ProtocolMessage::from_bytes(MessageCommand::Alert, &alert_bytes).unwrap();
        match decoded {
            ProtocolMessage::Alert(bytes) => assert_eq!(bytes, alert_bytes),
            other => panic!("expected Alert, got {other:?}"),
        }
    }

    #[test]
    fn unknown_command_preserves_bytes() {
        let data = vec![0xBB, 0xCC];
        let decoded =
            ProtocolMessage::from_bytes(MessageCommand::Unknown(0xFE), &data).unwrap();
        match decoded {
            ProtocolMessage::Unknown { command, bytes } => {
                assert_eq!(command, MessageCommand::Unknown(0xFE));
                assert_eq!(bytes, data);
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn ensure_empty_rejects_non_empty_payload() {
        let err = ProtocolMessage::from_bytes(MessageCommand::FilterClear, &[0x01]).unwrap_err();
        assert!(matches!(err, P2PMessageError::InvalidMessage(msg) if msg.contains("does not carry a payload")));
    }

    #[test]
    fn ping_payload_roundtrips() {
        let ping = PingPayload::create_with_nonce(42, 7);
        let msg = ProtocolMessage::Ping(ping);
        let bytes = msg.to_bytes().unwrap();

        let decoded = ProtocolMessage::from_bytes(MessageCommand::Ping, &bytes).unwrap();
        match decoded {
            ProtocolMessage::Ping(p) => {
                assert_eq!(p.last_block_index, 42);
                assert_eq!(p.nonce, 7);
            }
            other => panic!("expected Ping, got {other:?}"),
        }
    }

    #[test]
    fn network_message_roundtrips_empty_payload() {
        let original = NetworkMessage::new(ProtocolMessage::Verack);
        let bytes = original.to_bytes(true).unwrap();
        let decoded = NetworkMessage::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.command(), MessageCommand::Verack);
        assert!(matches!(decoded.payload, ProtocolMessage::Verack));
    }

    #[test]
    fn network_message_from_bytes_rejects_short_input() {
        let err = NetworkMessage::from_bytes(&[0x00]).unwrap_err();
        assert!(matches!(err, P2PMessageError::InvalidMessage(_)));
    }

    #[test]
    fn pong_convenience_constructor() {
        let pong = ProtocolMessage::pong_with_block_index(100, 55);
        match pong {
            ProtocolMessage::Pong(p) => {
                assert_eq!(p.last_block_index, 100);
                assert_eq!(p.nonce, 55);
            }
            _ => panic!("expected Pong"),
        }
    }

    #[test]
    fn should_try_compress_respects_unknown_variant() {
        let unknown = ProtocolMessage::Unknown {
            command: MessageCommand::Block,
            bytes: vec![0; 1024],
        };
        assert!(!unknown.should_try_compress());

        let block = ProtocolMessage::Block(Block::new());
        assert!(block.should_try_compress());
    }
}
