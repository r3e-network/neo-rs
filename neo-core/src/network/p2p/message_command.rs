//! Message command identifiers (mirrors `Neo.Network.P2P.MessageCommand`).

use crate::NetworkError;
use serde::{de::Error as DeError, Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, net::SocketAddr, str::FromStr};

/// Neo message command (single-byte discriminator).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MessageCommand {
    /// Version handshake message.
    Version,
    /// Version acknowledgment message.
    Verack,
    /// Request for peer addresses.
    GetAddr,
    /// Response with peer addresses.
    Addr,
    /// Ping message for keepalive.
    Ping,
    /// Pong response to ping.
    Pong,
    /// Request for block headers.
    GetHeaders,
    /// Response with block headers.
    Headers,
    /// Request for block hashes.
    GetBlocks,
    /// Request for mempool transactions.
    Mempool,
    /// Inventory announcement.
    Inv,
    /// Request for specific data.
    GetData,
    /// Request block by index.
    GetBlockByIndex,
    /// Data not found response.
    NotFound,
    /// Transaction payload.
    Transaction,
    /// Block payload.
    Block,
    /// Extensible message payload.
    Extensible,
    /// Rejection message.
    Reject,
    /// Load bloom filter.
    FilterLoad,
    /// Add to bloom filter.
    FilterAdd,
    /// Clear bloom filter.
    FilterClear,
    /// Merkle block for SPV.
    MerkleBlock,
    /// Alert message.
    Alert,
    /// Command value that is not recognised by this implementation.
    Unknown(u8),
}

impl MessageCommand {
    /// Returns the wire-format byte associated with the command.
    pub fn to_byte(self) -> u8 {
        match self {
            Self::Version => 0x00,
            Self::Verack => 0x01,
            Self::GetAddr => 0x10,
            Self::Addr => 0x11,
            Self::Ping => 0x18,
            Self::Pong => 0x19,
            Self::GetHeaders => 0x20,
            Self::Headers => 0x21,
            Self::GetBlocks => 0x24,
            Self::Mempool => 0x25,
            Self::Inv => 0x27,
            Self::GetData => 0x28,
            Self::GetBlockByIndex => 0x29,
            Self::NotFound => 0x2a,
            Self::Transaction => 0x2b,
            Self::Block => 0x2c,
            Self::Extensible => 0x2e,
            Self::Reject => 0x2f,
            Self::FilterLoad => 0x30,
            Self::FilterAdd => 0x31,
            Self::FilterClear => 0x32,
            Self::MerkleBlock => 0x38,
            Self::Alert => 0x40,
            Self::Unknown(value) => value,
        }
    }

    /// Alias for [`Self::to_byte`]; retained for backward compatibility.
    pub fn as_byte(self) -> u8 {
        self.to_byte()
    }

    /// Creates a command value from its byte representation.
    pub fn from_byte(byte: u8) -> Result<Self, NetworkError> {
        match byte {
            0x00 => Ok(Self::Version),
            0x01 => Ok(Self::Verack),
            0x10 => Ok(Self::GetAddr),
            0x11 => Ok(Self::Addr),
            0x18 => Ok(Self::Ping),
            0x19 => Ok(Self::Pong),
            0x20 => Ok(Self::GetHeaders),
            0x21 => Ok(Self::Headers),
            0x24 => Ok(Self::GetBlocks),
            0x25 => Ok(Self::Mempool),
            0x27 => Ok(Self::Inv),
            0x28 => Ok(Self::GetData),
            0x29 => Ok(Self::GetBlockByIndex),
            0x2a => Ok(Self::NotFound),
            0x2b => Ok(Self::Transaction),
            0x2c => Ok(Self::Block),
            0x2e => Ok(Self::Extensible),
            0x2f => Ok(Self::Reject),
            0x30 => Ok(Self::FilterLoad),
            0x31 => Ok(Self::FilterAdd),
            0x32 => Ok(Self::FilterClear),
            0x38 => Ok(Self::MerkleBlock),
            0x40 => Ok(Self::Alert),
            other => Ok(Self::Unknown(other)),
        }
    }

    /// Returns the canonical string representation used by the Neo protocol.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Version => "version",
            Self::Verack => "verack",
            Self::GetAddr => "getaddr",
            Self::Addr => "addr",
            Self::Ping => "ping",
            Self::Pong => "pong",
            Self::GetHeaders => "getheaders",
            Self::Headers => "headers",
            Self::GetBlocks => "getblocks",
            Self::Mempool => "mempool",
            Self::Inv => "inv",
            Self::GetData => "getdata",
            Self::GetBlockByIndex => "getblkbyidx",
            Self::NotFound => "notfound",
            Self::Transaction => "tx",
            Self::Block => "block",
            Self::Extensible => "extensible",
            Self::Reject => "reject",
            Self::FilterLoad => "filterload",
            Self::FilterAdd => "filteradd",
            Self::FilterClear => "filterclear",
            Self::MerkleBlock => "merkleblock",
            Self::Alert => "alert",
            Self::Unknown(_) => "unknown",
        }
    }

    /// Parses a command from its textual form.
    pub fn parse_str(s: &str) -> Result<Self, NetworkError> {
        match s {
            "version" => Ok(Self::Version),
            "verack" => Ok(Self::Verack),
            "getaddr" => Ok(Self::GetAddr),
            "addr" => Ok(Self::Addr),
            "ping" => Ok(Self::Ping),
            "pong" => Ok(Self::Pong),
            "getheaders" => Ok(Self::GetHeaders),
            "headers" => Ok(Self::Headers),
            "getblocks" => Ok(Self::GetBlocks),
            "mempool" => Ok(Self::Mempool),
            "inv" => Ok(Self::Inv),
            "getdata" => Ok(Self::GetData),
            "getblkbyidx" => Ok(Self::GetBlockByIndex),
            "notfound" => Ok(Self::NotFound),
            "tx" => Ok(Self::Transaction),
            "block" => Ok(Self::Block),
            "extensible" => Ok(Self::Extensible),
            "reject" => Ok(Self::Reject),
            "filterload" => Ok(Self::FilterLoad),
            "filteradd" => Ok(Self::FilterAdd),
            "filterclear" => Ok(Self::FilterClear),
            "merkleblock" => Ok(Self::MerkleBlock),
            "alert" => Ok(Self::Alert),
            "versionwithpayload" => Ok(Self::Unknown(0x55)),
            "extended83" => Ok(Self::Unknown(0x83)),
            "extended85" => Ok(Self::Unknown(0x85)),
            "extended86" => Ok(Self::Unknown(0x86)),
            "extendedbb" => Ok(Self::Unknown(0xbb)),
            "extendedbd" => Ok(Self::Unknown(0xbd)),
            "extendedbf" => Ok(Self::Unknown(0xbf)),
            "extendedc0" => Ok(Self::Unknown(0xc0)),
            "unknown" => Ok(Self::Unknown(0xff)),

            other => Err(NetworkError::ProtocolViolation {
                peer: SocketAddr::from(([0, 0, 0, 0], 0)),
                violation: format!("Unknown message command: {}", other),
            }),
        }
    }

    /// Returns `true` when the command is part of the official Neo enumeration.
    pub fn is_known(self) -> bool {
        !matches!(self, Self::Unknown(_))
    }
}

impl fmt::Display for MessageCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for MessageCommand {
    type Err = NetworkError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        MessageCommand::parse_str(s)
    }
}

impl Serialize for MessageCommand {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(self.to_byte())
    }
}

impl<'de> Deserialize<'de> for MessageCommand {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u8::deserialize(deserializer)?;
        MessageCommand::from_byte(value).map_err(D::Error::custom)
    }
}
