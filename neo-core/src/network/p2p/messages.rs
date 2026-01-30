//! Message framing and serialization (mirrors `Neo.Network.P2P.Message`).

use super::{
    message::PAYLOAD_MAX_SIZE,
    message_command::MessageCommand,
    message_flags::MessageFlags,
    payloads::{
        AddrPayload, Block, ExtensiblePayload, FilterAddPayload, FilterLoadPayload,
        GetBlockByIndexPayload, GetBlocksPayload, HeadersPayload, InvPayload, MerkleBlockPayload,
        PingPayload, Transaction, VersionPayload,
    },
};
use crate::compression::{
    compress_lz4, decompress_lz4, CompressionError, COMPRESSION_MIN_SIZE, COMPRESSION_THRESHOLD,
};
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
        if payload_bytes.len() > PAYLOAD_MAX_SIZE {
            return Err(NetworkError::InvalidMessage(format!(
                "Payload exceeds maximum size ({} > {})",
                payload_bytes.len(),
                PAYLOAD_MAX_SIZE
            )));
        }

        let should_compress = allow_compression
            && self.payload.should_try_compress()
            && payload_bytes.len() >= COMPRESSION_MIN_SIZE;

        let (final_payload, flags) = if should_compress {
            if let Ok(compressed) = compress_lz4(&payload_bytes) {
                // Honour the threshold check from the C# implementation.
                if compressed.len() + COMPRESSION_THRESHOLD < payload_bytes.len() {
                    (compressed, MessageFlags::COMPRESSED)
                } else {
                    (payload_bytes, MessageFlags::NONE)
                }
            } else {
                (payload_bytes, MessageFlags::NONE)
            }
        } else {
            (payload_bytes, MessageFlags::NONE)
        };

        // Calculate exact capacity needed: 1 (flags) + 1 (command) + varint + payload
        let varint_size = Self::calc_varint_size(final_payload.len());
        let total_size = 1 + 1 + varint_size + final_payload.len();
        let mut writer = BinaryWriter::with_capacity(total_size);

        writer.write_u8(flags.to_byte()).map_err(map_io_error)?;
        writer
            .write_u8(self.header.command.to_byte())
            .map_err(map_io_error)?;
        writer
            .write_var_bytes(&final_payload)
            .map_err(map_io_error)?;
        Ok(writer.into_bytes())
    }

    /// Calculates the size of a var_int encoding for the given value.
    const fn calc_varint_size(value: usize) -> usize {
        if value < 0xFD {
            1
        } else if value <= 0xFFFF {
            3
        } else if value <= 0xFFFF_FFFF {
            5
        } else {
            9
        }
    }

    /// Decodes a message that was previously produced by [`Self::to_bytes`].
    pub fn from_bytes(bytes: &[u8]) -> NetworkResult<Self> {
        if bytes.len() < 2 {
            return Err(NetworkError::InvalidMessage(
                "Message is too short (missing header)".to_string(),
            ));
        }

        let mut reader = MemoryReader::new(bytes);
        let flags_byte = reader.read_u8().map_err(map_io_error)?;
        let command_byte = reader.read_u8().map_err(map_io_error)?;

        let flags = MessageFlags::from_byte(flags_byte)?;
        let command = MessageCommand::from_byte(command_byte)?;

        let payload_len = reader
            .read_var_int(PAYLOAD_MAX_SIZE as u64)
            .map_err(map_io_error)? as usize;
        let payload_raw = reader.read_bytes(payload_len).map_err(map_io_error)?;
        let wire_payload = payload_raw.clone();

        if reader.remaining() != 0 {
            return Err(NetworkError::InvalidMessage(
                "Trailing data detected after payload".to_string(),
            ));
        }

        let payload_data = if flags.is_compressed() {
            decompress_lz4(&payload_raw, PAYLOAD_MAX_SIZE).map_err(map_compression_error)?
        } else {
            payload_raw
        };

        let payload = ProtocolMessage::deserialize(command, &payload_data)?;

        Ok(Self {
            header: MessageHeader { command },
            flags,
            payload,
            wire_payload: Some(wire_payload),
        })
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

impl ProtocolMessage {
    /// Convenience constructor for pong replies.
    pub fn pong(nonce: u32) -> Self {
        Self::pong_with_block_index(0, nonce)
    }

    /// Creates a pong reply with a specific block index and nonce.
    pub fn pong_with_block_index(block_index: u32, nonce: u32) -> Self {
        Self::Pong(PingPayload::create_with_nonce(block_index, nonce))
    }

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

    fn should_try_compress(&self) -> bool {
        matches!(
            self,
            Self::Block(_)
                | Self::Extensible(_)
                | Self::Transaction(_)
                | Self::Headers(_)
                | Self::Addr(_)
                | Self::MerkleBlock(_)
                | Self::FilterLoad(_)
                | Self::FilterAdd(_)
        )
    }

    /// Serializes the payload into its binary representation.
    pub fn to_bytes(&self) -> NetworkResult<Vec<u8>> {
        self.serialize()
    }

    /// Reconstructs the payload from its binary form and command discriminator.
    pub fn from_bytes(command: MessageCommand, data: &[u8]) -> NetworkResult<Self> {
        Self::deserialize(command, data)
    }

    fn serialize(&self) -> NetworkResult<Vec<u8>> {
        match self {
            Self::Version(payload) => serialize_payload(payload),
            Self::Verack | Self::GetAddr | Self::Mempool | Self::FilterClear => Ok(Vec::new()),
            Self::Addr(payload) => serialize_payload(payload),
            Self::Ping(payload) | Self::Pong(payload) => serialize_payload(payload),
            Self::GetHeaders(payload) => serialize_payload(payload),
            Self::GetBlockByIndex(payload) => serialize_payload(payload),
            Self::Headers(payload) => serialize_payload(payload),
            Self::GetBlocks(payload) => serialize_payload(payload),
            Self::Inv(payload) | Self::GetData(payload) | Self::NotFound(payload) => {
                serialize_payload(payload)
            }
            Self::Transaction(payload) => serialize_payload(payload),
            Self::Block(payload) => serialize_payload(payload),
            Self::Extensible(payload) => serialize_payload(payload),
            Self::FilterLoad(payload) => serialize_payload(payload),
            Self::FilterAdd(payload) => serialize_payload(payload),
            Self::MerkleBlock(payload) => serialize_payload(payload),
            Self::Alert(bytes) | Self::Reject(bytes) => Ok(bytes.clone()),
            Self::Unknown { bytes, .. } => Ok(bytes.clone()),
        }
    }

    fn deserialize(command: MessageCommand, data: &[u8]) -> NetworkResult<Self> {
        let message = match command {
            MessageCommand::Version => {
                let payload = deserialize_payload::<VersionPayload>(data)?;
                Self::Version(payload)
            }
            MessageCommand::Verack => ensure_empty(command, data).map(|_| Self::Verack)?,
            MessageCommand::GetAddr => ensure_empty(command, data).map(|_| Self::GetAddr)?,
            MessageCommand::Addr => {
                let payload = deserialize_payload::<AddrPayload>(data)?;
                Self::Addr(payload)
            }
            MessageCommand::Ping => {
                let payload = deserialize_payload::<PingPayload>(data)?;
                Self::Ping(payload)
            }
            MessageCommand::Pong => {
                let payload = deserialize_payload::<PingPayload>(data)?;
                Self::Pong(payload)
            }
            MessageCommand::GetHeaders | MessageCommand::GetBlockByIndex => {
                let payload = deserialize_payload::<GetBlockByIndexPayload>(data)?;
                if command == MessageCommand::GetHeaders {
                    Self::GetHeaders(payload)
                } else {
                    Self::GetBlockByIndex(payload)
                }
            }
            MessageCommand::Headers => {
                let payload = deserialize_payload::<HeadersPayload>(data)?;
                Self::Headers(payload)
            }
            MessageCommand::GetBlocks => {
                let payload = deserialize_payload::<GetBlocksPayload>(data)?;
                Self::GetBlocks(payload)
            }
            MessageCommand::Mempool => ensure_empty(command, data).map(|_| Self::Mempool)?,
            MessageCommand::Inv => {
                let payload = deserialize_payload::<InvPayload>(data)?;
                Self::Inv(payload)
            }
            MessageCommand::GetData => {
                let payload = deserialize_payload::<InvPayload>(data)?;
                Self::GetData(payload)
            }
            MessageCommand::NotFound => {
                let payload = deserialize_payload::<InvPayload>(data)?;
                Self::NotFound(payload)
            }
            MessageCommand::Transaction => {
                let payload = deserialize_payload::<Transaction>(data)?;
                Self::Transaction(payload)
            }
            MessageCommand::Block => {
                let payload = deserialize_payload::<Block>(data)?;
                Self::Block(payload)
            }
            MessageCommand::Extensible => {
                let payload = deserialize_payload::<ExtensiblePayload>(data)?;
                Self::Extensible(payload)
            }
            MessageCommand::Reject => Self::Reject(data.to_vec()),
            MessageCommand::FilterLoad => {
                let payload = deserialize_payload::<FilterLoadPayload>(data)?;
                Self::FilterLoad(payload)
            }
            MessageCommand::FilterAdd => {
                let payload = deserialize_payload::<FilterAddPayload>(data)?;
                Self::FilterAdd(payload)
            }
            MessageCommand::FilterClear => {
                ensure_empty(command, data).map(|_| Self::FilterClear)?
            }
            MessageCommand::MerkleBlock => {
                let payload = deserialize_payload::<MerkleBlockPayload>(data)?;
                Self::MerkleBlock(payload)
            }
            MessageCommand::Alert => Self::Alert(data.to_vec()),
            MessageCommand::Unknown(value) => Self::Unknown {
                command: MessageCommand::Unknown(value),
                bytes: data.to_vec(),
            },
        };

        Ok(message)
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

fn map_compression_error(error: CompressionError) -> NetworkError {
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
