//! Message command identifiers (mirrors `Neo.Network.P2P.MessageCommand`).

use crate::P2PError;
use neo_primitives::protocol_enum_with_unknown;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

protocol_enum_with_unknown! {
    /// Neo message command (single-byte discriminator).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub MessageCommand {
        unknown
        /// Command value that is not recognised by this implementation.
        Unknown(u8) => "unknown";

        /// Version handshake message
        Version = 0x00 => "version",
        /// Version acknowledgment
        Verack = 0x01 => "verack",
        /// Request peer addresses
        GetAddr = 0x10 => "getaddr",
        /// Peer address list
        Addr = 0x11 => "addr",
        /// Ping message for keepalive
        Ping = 0x18 => "ping",
        /// Pong response to ping
        Pong = 0x19 => "pong",
        /// Request block headers
        GetHeaders = 0x20 => "getheaders",
        /// Block headers response
        Headers = 0x21 => "headers",
        /// Request block hashes
        GetBlocks = 0x24 => "getblocks",
        /// Request mempool contents
        Mempool = 0x25 => "mempool",
        /// Inventory announcement
        Inv = 0x27 => "inv",
        /// Request specific data
        GetData = 0x28 => "getdata",
        /// Request block by index
        GetBlockByIndex = 0x29 => "getblkbyidx",
        /// Data not found response
        NotFound = 0x2a => "notfound",
        /// Transaction payload
        Transaction = 0x2b => "tx",
        /// Block payload
        Block = 0x2c => "block",
        /// Extensible payload (consensus, state root, etc.)
        Extensible = 0x2e => "extensible",
        /// Rejection message
        Reject = 0x2f => "reject",
        /// Load bloom filter
        FilterLoad = 0x30 => "filterload",
        /// Add to bloom filter
        FilterAdd = 0x31 => "filteradd",
        /// Clear bloom filter
        FilterClear = 0x32 => "filterclear",
        /// Merkle block for SPV
        MerkleBlock = 0x38 => "merkleblock",
        /// Alert message
        Alert = 0x40 => "alert",
    }
}

impl MessageCommand {
    /// Parses a command from its textual form.
    pub fn parse_str(s: &str) -> Result<Self, P2PError> {
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
            "unknown" => Ok(Self::Unknown(0xff)),
            other => Err(P2PError::protocol_error(format!(
                "Unknown message command: {}",
                other
            ))),
        }
    }
}

impl fmt::Display for MessageCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for MessageCommand {
    type Err = P2PError;

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
        Ok(MessageCommand::from_byte(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_command_byte_values() {
        assert_eq!(MessageCommand::Version.to_byte(), 0x00);
        assert_eq!(MessageCommand::Verack.to_byte(), 0x01);
        assert_eq!(MessageCommand::Transaction.to_byte(), 0x2b);
        assert_eq!(MessageCommand::Block.to_byte(), 0x2c);
        assert_eq!(MessageCommand::Extensible.to_byte(), 0x2e);
    }

    #[test]
    fn test_message_command_from_byte() {
        assert_eq!(MessageCommand::from_byte(0x00), MessageCommand::Version);
        assert_eq!(MessageCommand::from_byte(0x2b), MessageCommand::Transaction);
        assert_eq!(
            MessageCommand::from_byte(0x99),
            MessageCommand::Unknown(0x99)
        );
    }

    #[test]
    fn protocol_enum_guard_preserves_unknown_message_command_bytes() {
        let command = MessageCommand::from_byte(0x99);
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

    #[test]
    fn test_message_command_as_str() {
        assert_eq!(MessageCommand::Version.as_str(), "version");
        assert_eq!(MessageCommand::Transaction.as_str(), "tx");
        assert_eq!(MessageCommand::Block.as_str(), "block");
    }

    #[test]
    fn test_message_command_parse_str() {
        assert_eq!(
            MessageCommand::parse_str("version").unwrap(),
            MessageCommand::Version
        );
        assert_eq!(
            MessageCommand::parse_str("tx").unwrap(),
            MessageCommand::Transaction
        );
        assert!(MessageCommand::parse_str("invalid").is_err());
    }

    #[test]
    fn test_message_command_is_known() {
        assert!(MessageCommand::Version.is_known());
        assert!(MessageCommand::Block.is_known());
        assert!(!MessageCommand::Unknown(0x99).is_known());
    }

    #[test]
    fn test_message_command_roundtrip() {
        for cmd in [
            MessageCommand::Version,
            MessageCommand::Verack,
            MessageCommand::Transaction,
            MessageCommand::Block,
            MessageCommand::Extensible,
        ] {
            let byte = cmd.to_byte();
            let recovered = MessageCommand::from_byte(byte);
            assert_eq!(cmd, recovered);
        }
    }
}
