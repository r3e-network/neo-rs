use super::payload_codec::{deserialize_payload, ensure_empty, serialize_payload};
use crate::network::NetworkResult;
use crate::network::p2p::message::should_try_compress_command;
use crate::network::p2p::message_command::MessageCommand;
use crate::network::p2p::payloads::{
    AddrPayload, Block, ExtensiblePayload, FilterAddPayload, FilterLoadPayload,
    GetBlockByIndexPayload, GetBlocksPayload, HeadersPayload, InvPayload, MerkleBlockPayload,
    PingPayload, Transaction, VersionPayload,
};

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

    pub(super) fn should_try_compress(&self) -> bool {
        should_try_compress_command(self.command())
    }

    /// Serializes the payload into its binary representation.
    pub fn to_bytes(&self) -> NetworkResult<Vec<u8>> {
        self.serialize()
    }

    /// Reconstructs the payload from its binary form and command discriminator.
    pub fn from_bytes(command: MessageCommand, data: &[u8]) -> NetworkResult<Self> {
        Self::deserialize(command, data)
    }

    pub(super) fn serialize(&self) -> NetworkResult<Vec<u8>> {
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

    pub(super) fn deserialize(command: MessageCommand, data: &[u8]) -> NetworkResult<Self> {
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
