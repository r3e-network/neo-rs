//! Message command identifiers (mirrors `Neo.Network.P2P.MessageCommand`).

use neo_primitives::InventoryType;
use std::{fmt, str::FromStr};

neo_primitives::protocol_enum_with_unknown! {
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

/// Error returned when a textual P2P message command is unknown.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Unknown message command: {0}")]
pub struct MessageCommandParseError(pub String);

impl fmt::Display for MessageCommand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for MessageCommand {
    type Err = MessageCommandParseError;

    fn from_str(command: &str) -> Result<Self, Self::Err> {
        Self::parse_str(command)
    }
}

impl serde::Serialize for MessageCommand {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u8(self.to_byte())
    }
}

impl<'de> serde::Deserialize<'de> for MessageCommand {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let byte = <u8 as serde::Deserialize>::deserialize(deserializer)?;
        Ok(Self::from_byte(byte))
    }
}

impl From<InventoryType> for MessageCommand {
    fn from(inventory_type: InventoryType) -> Self {
        match inventory_type {
            InventoryType::Transaction => Self::Transaction,
            InventoryType::Block => Self::Block,
            InventoryType::Extensible => Self::Extensible,
        }
    }
}

impl MessageCommand {
    const KNOWN_COMMANDS: [Self; 23] = [
        Self::Version,
        Self::Verack,
        Self::GetAddr,
        Self::Addr,
        Self::Ping,
        Self::Pong,
        Self::GetHeaders,
        Self::Headers,
        Self::GetBlocks,
        Self::Mempool,
        Self::Inv,
        Self::GetData,
        Self::GetBlockByIndex,
        Self::NotFound,
        Self::Transaction,
        Self::Block,
        Self::Extensible,
        Self::Reject,
        Self::FilterLoad,
        Self::FilterAdd,
        Self::FilterClear,
        Self::MerkleBlock,
        Self::Alert,
    ];

    /// Creates a command value from its byte representation.
    ///
    /// Unknown bytes are preserved so private or future commands can round-trip.
    pub const fn from_byte(byte: u8) -> Self {
        Self::from_byte_unchecked(byte)
    }

    /// Parses a command from its canonical textual form.
    pub fn parse_str(command: &str) -> Result<Self, MessageCommandParseError> {
        if let Some(known) = Self::KNOWN_COMMANDS
            .into_iter()
            .find(|known| command == known.as_str())
        {
            return Ok(known);
        }

        let parsed = match command {
            "unknown" => Self::Unknown(0xff),
            "versionwithpayload" => Self::Unknown(0x55),
            "extended83" => Self::Unknown(0x83),
            "extended85" => Self::Unknown(0x85),
            "extended86" => Self::Unknown(0x86),
            "extendedbb" => Self::Unknown(0xbb),
            "extendedbd" => Self::Unknown(0xbd),
            "extendedbf" => Self::Unknown(0xbf),
            "extendedc0" => Self::Unknown(0xc0),
            other => return Err(MessageCommandParseError(other.to_owned())),
        };
        Ok(parsed)
    }

    /// Returns true when C# Neo permits attempting LZ4 compression for this command.
    pub const fn allows_compression(self) -> bool {
        matches!(
            self,
            Self::Block
                | Self::Extensible
                | Self::Transaction
                | Self::Headers
                | Self::Addr
                | Self::MerkleBlock
                | Self::FilterLoad
                | Self::FilterAdd
        )
    }

    /// Commands that should only have one item queued at a time.
    pub const SINGLE_QUEUED_COMMANDS: [Self; 7] = [
        Self::Addr,
        Self::GetAddr,
        Self::GetBlocks,
        Self::GetHeaders,
        Self::Mempool,
        Self::Ping,
        Self::Pong,
    ];

    /// Commands that should be processed with high priority.
    pub const HIGH_PRIORITY_COMMANDS: [Self; 7] = [
        Self::Alert,
        Self::Extensible,
        Self::FilterAdd,
        Self::FilterClear,
        Self::FilterLoad,
        Self::GetAddr,
        Self::Mempool,
    ];

    /// Returns true if this command should only have one item queued.
    pub fn is_single_queued(self) -> bool {
        Self::SINGLE_QUEUED_COMMANDS.contains(&self)
    }

    /// Returns true if this command should be processed with high priority.
    pub fn is_high_priority_queue(self) -> bool {
        Self::HIGH_PRIORITY_COMMANDS.contains(&self)
    }
}

#[cfg(test)]
#[path = "../tests/proto/message_command.rs"]
mod tests;
