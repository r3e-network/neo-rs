//! Message command identifiers (mirrors `Neo.Network.P2P.MessageCommand`).

use crate::NetworkError;
use neo_primitives::protocol_enum_with_unknown;
use serde::{de::Error as DeError, Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, net::SocketAddr, str::FromStr};

protocol_enum_with_unknown! {
    /// Neo message command (single-byte discriminator).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub MessageCommand {
        from_byte = from_byte_unchecked;
        unknown
        /// Command value that is not recognised by this implementation.
        Unknown(u8) => "unknown";

        /// Version handshake message.
        Version = 0x00 => "version",
        /// Version acknowledgment message.
        Verack = 0x01 => "verack",
        /// Request for peer addresses.
        GetAddr = 0x10 => "getaddr",
        /// Response with peer addresses.
        Addr = 0x11 => "addr",
        /// Ping message for keepalive.
        Ping = 0x18 => "ping",
        /// Pong response to ping.
        Pong = 0x19 => "pong",
        /// Request for block headers.
        GetHeaders = 0x20 => "getheaders",
        /// Response with block headers.
        Headers = 0x21 => "headers",
        /// Request for block hashes.
        GetBlocks = 0x24 => "getblocks",
        /// Request for mempool transactions.
        Mempool = 0x25 => "mempool",
        /// Inventory announcement.
        Inv = 0x27 => "inv",
        /// Request for specific data.
        GetData = 0x28 => "getdata",
        /// Request block by index.
        GetBlockByIndex = 0x29 => "getblkbyidx",
        /// Data not found response.
        NotFound = 0x2a => "notfound",
        /// Transaction payload.
        Transaction = 0x2b => "tx",
        /// Block payload.
        Block = 0x2c => "block",
        /// Extensible message payload.
        Extensible = 0x2e => "extensible",
        /// Rejection message.
        Reject = 0x2f => "reject",
        /// Load bloom filter.
        FilterLoad = 0x30 => "filterload",
        /// Add to bloom filter.
        FilterAdd = 0x31 => "filteradd",
        /// Clear bloom filter.
        FilterClear = 0x32 => "filterclear",
        /// Merkle block for SPV.
        MerkleBlock = 0x38 => "merkleblock",
        /// Alert message.
        Alert = 0x40 => "alert",
    }
}

impl MessageCommand {
    /// Creates a command value from its byte representation.
    pub fn from_byte(byte: u8) -> Result<Self, NetworkError> {
        Ok(Self::from_byte_unchecked(byte))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_enum_guard_preserves_unknown_message_command_bytes() {
        let command = MessageCommand::from_byte(0x99).unwrap();
        assert_eq!(command, MessageCommand::Unknown(0x99));
        assert_eq!(command.to_byte(), 0x99);
        assert_eq!(command.as_byte(), 0x99);
        assert_eq!(command.as_str(), "unknown");
        assert!(!command.is_known());

        let serialized = serde_json::to_string(&command).unwrap();
        assert_eq!(serialized, "153");
        let deserialized: MessageCommand = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, command);
    }
}
