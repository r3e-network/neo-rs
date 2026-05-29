//! Message framing and serialization (mirrors `Neo.Network.P2P.Message`).

use super::{
    message::Message,
    MessageCommand,
    MessageFlags,
    payloads::{
        AddrPayload, Block, ExtensiblePayload, FilterAddPayload, FilterLoadPayload,
        GetBlockByIndexPayload, GetBlocksPayload, HeadersPayload, InvPayload, MerkleBlockPayload,
        PingPayload, Transaction, VersionPayload,
    },
};
#[cfg(test)]
use crate::compression::COMPRESSION_MIN_SIZE;
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use crate::network::{NetworkError, NetworkResult};

/// Header metadata attached to every P2P message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageHeader {
    /// Command opcode that identifies the payload type.
    pub command: MessageCommand,
}

/// Fully decoded network message.
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

    /// Returns the message encoded exactly as it would appear on the wire.
    ///
    /// `allow_compression` mirrors the C# `Message.ToArray(bool)` behaviour:
    /// when set to `false`, the payload is always emitted uncompressed even if
    /// it would normally satisfy the compression heuristics.
    ///
    /// Optimizations:
    /// - Pre-calculates buffer capacity to minimize reallocations
    /// - Single allocation for the output buffer
    pub fn to_bytes(&self, allow_compression: bool) -> NetworkResult<Vec<u8>> {
        let payload_bytes = self.payload.serialize()?;
        let message = Message::create_from_payload_bytes(
            self.header.command,
            payload_bytes,
            allow_compression && self.payload.should_try_compress(),
        )?;
        message.to_bytes(allow_compression).map_err(map_io_error)
    }

    /// Decodes a message that was previously produced by [`Self::to_bytes`].
    pub fn from_bytes(bytes: &[u8]) -> NetworkResult<Self> {
        let mut reader = MemoryReader::new(bytes);
        let message = <Message as Serializable>::deserialize(&mut reader).map_err(map_io_error)?;

        if reader.remaining() != 0 {
            return Err(NetworkError::InvalidMessage(
                "Trailing data detected after payload".to_string(),
            ));
        }

        let payload = message.to_protocol_message()?;

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

impl neo_primitives::NetworkMessage for NetworkMessage {
    fn command(&self) -> &str {
        self.header.command.as_str()
    }

    fn serialize(&self) -> Vec<u8> {
        self.to_bytes(true).unwrap_or_default()
    }
}

/// Strongly-typed representation of every payload carried by the Neo P2P
/// protocol.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum ProtocolMessage {
    Version(VersionPayload),
    Verack,
    GetAddr,
    Addr(AddrPayload),
    Ping(PingPayload),
    Pong(PingPayload),
    GetHeaders(GetBlockByIndexPayload),
    Headers(HeadersPayload),
    GetBlocks(GetBlocksPayload),
    Mempool,
    Inv(InvPayload),
    GetData(InvPayload),
    GetBlockByIndex(GetBlockByIndexPayload),
    NotFound(InvPayload),
    Transaction(Transaction),
    Block(Block),
    Extensible(ExtensiblePayload),
    FilterLoad(FilterLoadPayload),
    FilterAdd(FilterAddPayload),
    FilterClear,
    MerkleBlock(MerkleBlockPayload),
    Alert(Vec<u8>),
    Reject(Vec<u8>),
    Unknown {
        command: MessageCommand,
        bytes: Vec<u8>,
    },
}

macro_rules! impl_protocol_message_codecs {
    (
        typed { $($typed_variant:ident($typed_payload:ty) => $typed_command:ident;)+ }
        empty { $($empty_variant:ident => $empty_command:ident;)+ }
        raw { $($raw_variant:ident => $raw_command:ident;)+ }
    ) => {
        impl ProtocolMessage {
            /// Returns the underlying command associated with this payload.
            pub fn command(&self) -> MessageCommand {
                match self {
                    $(
                        Self::$typed_variant(_) => MessageCommand::$typed_command,
                    )+
                    $(
                        Self::$empty_variant => MessageCommand::$empty_command,
                    )+
                    $(
                        Self::$raw_variant(_) => MessageCommand::$raw_command,
                    )+
                    Self::Unknown { command, .. } => *command,
                }
            }

            fn serialize(&self) -> NetworkResult<Vec<u8>> {
                match self {
                    $(
                        Self::$typed_variant(payload) => serialize_payload(payload),
                    )+
                    $(
                        Self::$empty_variant => Ok(Vec::new()),
                    )+
                    $(
                        Self::$raw_variant(bytes) => Ok(bytes.clone()),
                    )+
                    Self::Unknown { bytes, .. } => Ok(bytes.clone()),
                }
            }

            fn deserialize(command: MessageCommand, data: &[u8]) -> NetworkResult<Self> {
                let message = match command {
                    $(
                        MessageCommand::$typed_command => {
                            let payload = deserialize_payload::<$typed_payload>(data)?;
                            Self::$typed_variant(payload)
                        }
                    )+
                    $(
                        MessageCommand::$empty_command => {
                            ensure_empty(command, data).map(|_| Self::$empty_variant)?
                        }
                    )+
                    $(
                        MessageCommand::$raw_command => Self::$raw_variant(data.to_vec()),
                    )+
                    MessageCommand::Unknown(value) => Self::Unknown {
                        command: MessageCommand::Unknown(value),
                        bytes: data.to_vec(),
                    },
                };

                Ok(message)
            }
        }
    };
}

impl_protocol_message_codecs! {
    typed {
        Version(VersionPayload) => Version;
        Addr(AddrPayload) => Addr;
        Ping(PingPayload) => Ping;
        Pong(PingPayload) => Pong;
        GetHeaders(GetBlockByIndexPayload) => GetHeaders;
        Headers(HeadersPayload) => Headers;
        GetBlocks(GetBlocksPayload) => GetBlocks;
        Inv(InvPayload) => Inv;
        GetData(InvPayload) => GetData;
        GetBlockByIndex(GetBlockByIndexPayload) => GetBlockByIndex;
        NotFound(InvPayload) => NotFound;
        Transaction(Transaction) => Transaction;
        Block(Block) => Block;
        Extensible(ExtensiblePayload) => Extensible;
        FilterLoad(FilterLoadPayload) => FilterLoad;
        FilterAdd(FilterAddPayload) => FilterAdd;
        MerkleBlock(MerkleBlockPayload) => MerkleBlock;
    }
    empty {
        Verack => Verack;
        GetAddr => GetAddr;
        Mempool => Mempool;
        FilterClear => FilterClear;
    }
    raw {
        Alert => Alert;
        Reject => Reject;
    }
}

impl ProtocolMessage {
    /// Convenience constructor for pong replies.
    pub fn pong(nonce: u32) -> Self {
        Self::pong_with_block_index(0, nonce)
    }

    /// Creates a pong reply with a specific block index and nonce.
    pub fn pong_with_block_index(block_index: u32, nonce: u32) -> Self {
        Self::Pong(PingPayload::create_with_nonce(block_index, nonce))
    }

    fn should_try_compress(&self) -> bool {
        !matches!(self, Self::Unknown { .. }) && self.command().allows_compression()
    }

    /// Serializes the payload into its binary representation.
    pub fn to_bytes(&self) -> NetworkResult<Vec<u8>> {
        self.serialize()
    }

    /// Reconstructs the payload from its binary form and command discriminator.
    pub fn from_bytes(command: MessageCommand, data: &[u8]) -> NetworkResult<Self> {
        Self::deserialize(command, data)
    }
}

fn serialize_payload<T>(payload: &T) -> NetworkResult<Vec<u8>>
where
    T: PayloadSerializable,
{
    payload
        .serialize_to_vec()
        .map_err(|e| NetworkError::InvalidMessage(e.to_string()))
}

type PayloadResult<T> = IoResult<T>;

fn deserialize_payload<T>(bytes: &[u8]) -> NetworkResult<T>
where
    T: PayloadDeserializable,
{
    let mut reader = MemoryReader::new(bytes);
    let payload =
        T::deserialize(&mut reader).map_err(|e| NetworkError::InvalidMessage(e.to_string()))?;
    if reader.remaining() != 0 {
        return Err(NetworkError::InvalidMessage(
            "Trailing bytes present after payload deserialization".to_string(),
        ));
    }
    Ok(payload)
}

fn ensure_empty(command: MessageCommand, bytes: &[u8]) -> NetworkResult<()> {
    if bytes.is_empty() {
        Ok(())
    } else {
        Err(NetworkError::InvalidMessage(format!(
            "Command {:?} does not carry a payload but {} byte(s) were provided",
            command,
            bytes.len()
        )))
    }
}

fn map_io_error(error: IoError) -> NetworkError {
    NetworkError::InvalidMessage(error.to_string())
}

trait PayloadSerializable {
    fn serialize_to_vec(&self) -> IoResult<Vec<u8>>;
}

trait PayloadDeserializable: Sized {
    fn deserialize(reader: &mut MemoryReader) -> PayloadResult<Self>;
}

macro_rules! impl_payload_codec {
    ($type:ty) => {
        impl PayloadSerializable for $type {
            fn serialize_to_vec(&self) -> IoResult<Vec<u8>> {
                let mut writer = BinaryWriter::new();
                Serializable::serialize(self, &mut writer)?;
                Ok(writer.into_bytes())
            }
        }

        impl PayloadDeserializable for $type {
            fn deserialize(reader: &mut MemoryReader) -> PayloadResult<Self> {
                <$type as Serializable>::deserialize(reader)
            }
        }
    };
}

// Implement the codec helpers for every payload that already satisfies the
// `crate::neo_io::Serializable` contract.
impl_payload_codec!(VersionPayload);
impl_payload_codec!(AddrPayload);
impl_payload_codec!(PingPayload);
impl_payload_codec!(GetBlockByIndexPayload);
impl_payload_codec!(HeadersPayload);
impl_payload_codec!(GetBlocksPayload);
impl_payload_codec!(InvPayload);
impl_payload_codec!(Transaction);
impl_payload_codec!(Block);
impl_payload_codec!(ExtensiblePayload);
impl_payload_codec!(FilterLoadPayload);
impl_payload_codec!(FilterAddPayload);
impl_payload_codec!(MerkleBlockPayload);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_protocol_message_never_uses_command_compression_whitelist() {
        let bytes = vec![0xAB; COMPRESSION_MIN_SIZE + 64];
        let message = NetworkMessage::new(ProtocolMessage::Unknown {
            command: MessageCommand::Block,
            bytes: bytes.clone(),
        });

        let encoded = message.to_bytes(true).unwrap();

        assert_eq!(encoded[0], MessageFlags::NONE.to_byte());
        assert_eq!(encoded[1], MessageCommand::Block.to_byte());
        assert!(encoded.ends_with(&bytes));
    }

    #[test]
    fn network_message_uses_strict_compression_min_size_threshold() {
        let payload =
            ProtocolMessage::FilterAdd(FilterAddPayload::new(vec![0xAB; COMPRESSION_MIN_SIZE - 1]));
        assert_eq!(payload.to_bytes().unwrap().len(), COMPRESSION_MIN_SIZE);
        let message = NetworkMessage::new(payload);

        let encoded = message.to_bytes(true).unwrap();

        assert_eq!(encoded[0], MessageFlags::NONE.to_byte());
        assert_eq!(encoded[1], MessageCommand::FilterAdd.to_byte());
    }

    #[test]
    fn protocol_message_serializes_empty_and_raw_payload_families() {
        for message in [
            ProtocolMessage::Verack,
            ProtocolMessage::GetAddr,
            ProtocolMessage::Mempool,
            ProtocolMessage::FilterClear,
        ] {
            assert!(message.to_bytes().unwrap().is_empty());
        }

        let alert = vec![0xA1, 0xA2];
        assert_eq!(
            ProtocolMessage::Alert(alert.clone()).to_bytes().unwrap(),
            alert
        );

        let reject = vec![0xB1, 0xB2];
        assert_eq!(
            ProtocolMessage::Reject(reject.clone()).to_bytes().unwrap(),
            reject
        );
    }

    #[test]
    fn protocol_message_deserializes_typed_empty_raw_and_unknown_families() {
        let ping = PingPayload::create_with_nonce(42, 7);
        let ping_bytes = serialize_payload(&ping).unwrap();

        match ProtocolMessage::from_bytes(MessageCommand::Ping, &ping_bytes).unwrap() {
            ProtocolMessage::Ping(decoded) => {
                assert_eq!(decoded.last_block_index, 42);
                assert_eq!(decoded.nonce, 7);
            }
            other => panic!("expected ping payload, got {other:?}"),
        }

        assert!(matches!(
            ProtocolMessage::from_bytes(MessageCommand::Verack, &[]).unwrap(),
            ProtocolMessage::Verack
        ));

        let alert = ProtocolMessage::from_bytes(MessageCommand::Alert, &[0xA1, 0xA2]).unwrap();
        assert!(matches!(alert, ProtocolMessage::Alert(bytes) if bytes == [0xA1, 0xA2]));

        let unknown = ProtocolMessage::from_bytes(MessageCommand::Unknown(0xFE), &[0xB1]).unwrap();
        assert!(
            matches!(unknown, ProtocolMessage::Unknown { command: MessageCommand::Unknown(0xFE), bytes } if bytes == [0xB1])
        );
    }

    #[test]
    fn protocol_message_rejects_non_empty_empty_payloads() {
        let error = ProtocolMessage::from_bytes(MessageCommand::FilterClear, &[0x01]).unwrap_err();

        assert!(
            matches!(error, NetworkError::InvalidMessage(message) if message.contains("does not carry a payload"))
        );
    }

    #[test]
    fn protocol_message_rejects_typed_payload_trailing_bytes() {
        let ping = PingPayload::create_with_nonce(42, 7);
        let mut ping_bytes = serialize_payload(&ping).unwrap();
        ping_bytes.push(0xFF);

        let error = ProtocolMessage::from_bytes(MessageCommand::Ping, &ping_bytes).unwrap_err();

        assert!(
            matches!(error, NetworkError::InvalidMessage(message) if message.contains("Trailing bytes present"))
        );
    }
}
